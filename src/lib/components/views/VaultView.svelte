<script lang="ts">
  // src/lib/components/views/VaultView.svelte
  //
  // 主窗口「资料库」视图（Task 13 收敛版）。
  //
  // 设计要点：
  //   * 单一 all-entries 数据源 + 前端 filter（all / credential / bookmark / note）。
  //   * 搜索由 HybridSearchController 驱动，混合本地 + AI 扩展。
  //   * 两行 header：
  //       Row 1: [搜索框 ............] [+ 新建]
  //       Row 2: [全部 N] [凭据 N] [书签 N] [笔记 N]
  //   * "+ 新建"打开可键盘操作的类型菜单（popover），不再并排三个按钮。
  //   * 删除走乐观 UI + 撤销 toast（不使用 browser confirm）。
  //   * AI 错误事件 → toast（不调 alert）。
  //   * 所有 listen() 在 onMount cleanup 中 unlisten；async listen 用
  //     disposed 标志保护。

  import { onMount } from 'svelte'
  import { listen, type UnlistenFn } from '@tauri-apps/api/event'
  import { vaultApi } from '$lib/api/vault'
  import { onTagsUpdated, onLlmError } from '$lib/api/vault'
  import { messages } from '$lib/i18n'
  import { HybridSearchController, type HybridSearchApi, type HybridSearchState } from '$lib/state/vault-search'
  import {
    LibraryViewController,
    type DeletePendingEntry,
    type LibraryFilter,
    type LibraryNotify,
  } from '$lib/state/library-view'
  import type {
    EntryKind,
    VaultEntryDetail,
    VaultEntryInput,
    VaultEntrySummary,
    VaultSearchHit,
  } from '$lib/types/vault'
  import EntryCard from '$lib/components/vault/EntryCard.svelte'
  import VaultEntryEditor from '$lib/components/vault/VaultEntryEditor.svelte'
  import LibrarySearchInput from '$lib/components/vault/LibrarySearchInput.svelte'

  interface Props {
    notify: LibraryNotify
  }

  let { notify }: Props = $props()

  // ---- state ----

  let loading = $state(false)
  let editorMode = $state<null | { mode: 'create'; kind: EntryKind } | { mode: 'edit'; id: string; detail: VaultEntryDetail }>(null)
  let showNewMenu = $state(false)
  let aiConfigured = $state(false)
  let autoHybridSearch = $state(false)

  // HybridSearchController needs an api object. Wire it to vaultApi so we
  // get real IPC behavior in production.
  const searchApi: HybridSearchApi = {
    searchLocal: (q, plan, limit) => vaultApi.searchLocal(q, plan, limit),
    planSearch: (q, requestId) => vaultApi.planSearch(q, requestId),
    cancelSearch: (requestId) => vaultApi.cancelSearch(requestId),
  }

  let hybridState = $state<HybridSearchState | null>(null)

  const searchController = new HybridSearchController({
    api: searchApi,
    delayMs: 700,
    onState: (s) => {
      hybridState = s
    },
  })

  // ---- library view controller ----
  //
  // allEntries 是 Svelte `$state`，作为渲染层的真相数据源。controller
  // 只读 snapshot + 维护 pendingDeletes / committingIds。删除/恢复通过
  // 回调修改本组件的 $state，保证 derived 能正确追踪。

  // 暴露给模板用的派生状态
  let allEntries = $state<VaultEntrySummary[]>([])
  let activeFilter = $state<LibraryFilter>('all')
  let searchStarted = $state(false)
  let searchHits = $state<VaultSearchHit[] | null>(null)

  /**
   * pendingDeleteVersion 用于在 $derived 中建立对 controller.pendingDeletes
   * 变化的依赖：每次 requestDelete/undoDelete/commitDelete 通过
   * $effect 同步到本 $state，进而触发 derived 重算。
   */
  let pendingVersion = $state(0)

  function bumpPendingVersion() {
    pendingVersion = ctrl.pendingDeleteVersion()
  }

  function restorePendingToAllEntries(pending: DeletePendingEntry) {
    // 重新插入到 allEntries 的对应位置；如果位置超出当前长度则插到末尾。
    const next = allEntries.slice()
    const insertAt = Math.min(pending.originalIndex, next.length)
    next.splice(insertAt, 0, pending.summary)
    allEntries = next
    ctrl.syncAllEntries(next)
  }

  const ctrl = new LibraryViewController({
    onDelete: (id) => vaultApi.deleteEntry(id),
    notify: (text, kind, undo, actionLabel) => notify(text, kind, undo, actionLabel),
    deleteDelayMs: 3000,
    onRestoreFailedDelete: (pending) => {
      restorePendingToAllEntries(pending)
      bumpPendingVersion()
    },
    onRestoreUndo: (pending) => {
      restorePendingToAllEntries(pending)
      bumpPendingVersion()
    },
  })

  // ---- data load ----

  async function reload() {
    loading = true
    try {
      const list = await vaultApi.listEntries()
      // I2: 过滤掉当前处于 pending-delete 窗口的条目，避免 reload
      // 把乐观删除的条目复活（3s 提交窗口内）。
      const filtered = list.filter((e) => !ctrl.isPendingDelete(e.entry.id))
      allEntries = filtered
      ctrl.setAllEntries(filtered)
    } finally {
      loading = false
    }
  }

  async function loadAiSettings() {
    try {
      const [cfg, settings] = await Promise.all([
        vaultApi.getLlmConfig(),
        vaultApi.getAiSettings(),
      ])
      aiConfigured = cfg !== null
      autoHybridSearch = cfg !== null && settings.autoHybridSearch
    } catch {
      // LLM 配置读取失败：保守地关闭 AI 搜索。
      aiConfigured = false
      autoHybridSearch = false
    }
  }

  // ---- derived ----
  //
  // C1 fix: derived 直接读 $state（allEntries / pendingVersion / activeFilter），
  // 这样 Svelte 5 能正确追踪变化。pendingVersion 是 $state，由
  // handleDelete / undo / commit-fail / commit-success 显式 bump。
  // controller 是 plain class，其内部字段不会被 Svelte 追踪。

  const counts = $derived.by(() => {
    // 读 pendingVersion 建立 reactivity 依赖。
    void pendingVersion
    return ctrl.countsFrom(allEntries)
  })

  // 当处于搜索态时使用 hits；否则使用 allEntries 按 filter 过滤。
  const visibleSummaries = $derived.by<VaultEntrySummary[]>(() => {
    // 触发依赖追踪
    void pendingVersion
    if (searchStarted && searchHits !== null) {
      return searchHits
        .map((h) => h.summary)
        .filter((s) => !ctrl.isPendingDelete(s.entry.id))
    }
    return ctrl.filterEntries(allEntries, activeFilter)
  })

  const emptyState = $derived.by<{ kind: 'loading' | 'empty' | 'no-results' | 'list' }>(() => {
    if (loading) return { kind: 'loading' as const }
    if (searchStarted && searchHits !== null) {
      if (searchHits.length === 0) return { kind: 'no-results' as const }
      return { kind: 'list' as const }
    }
    if (allEntries.length === 0) return { kind: 'empty' as const }
    return { kind: 'list' as const }
  })

  // ---- handlers ----

  function selectFilter(f: LibraryFilter) {
    activeFilter = f
  }

  function onSearchStarted(started: boolean) {
    searchStarted = started
    if (!started) {
      searchHits = null
    }
  }

  function onSearchQueryChange(_q: string) {
    // query 现在由 HybridSearchController 自己持有；VaultView 不再镜像。
  }

  // hybridState 变化时把 hits 同步到 $state
  $effect(() => {
    if (!hybridState) return
    if (!searchStarted) return
    const hits = hybridState.hits
    searchHits = hits.slice()
  })

  function openNewMenu() {
    showNewMenu = true
  }
  function closeNewMenu() {
    showNewMenu = false
  }
  function startCreate(kind: EntryKind) {
    showNewMenu = false
    editorMode = { mode: 'create', kind }
  }

  async function startEdit(id: string) {
    try {
      const detail = await vaultApi.getEntry(id)
      editorMode = { mode: 'edit', id, detail }
    } catch (e) {
      const msg = e instanceof Error && e.message ? e.message : String(e)
      notify(`${messages.toast.loadFailed}: ${msg}`, 'error')
    }
  }

  function cancelEditor() {
    editorMode = null
  }

  async function handleSaveCreate(input: VaultEntryInput) {
    try {
      await vaultApi.createEntry(input)
      editorMode = null
      await reload()
      notify(messages.library.created, 'success')
    } catch (e) {
      const msg = e instanceof Error && e.message ? e.message : String(e)
      notify(`${messages.toast.createFailed}: ${msg}`, 'error')
    }
  }

  async function handleSaveEdit(input: VaultEntryInput) {
    if (!editorMode || editorMode.mode !== 'edit') return
    try {
      await vaultApi.updateEntry(editorMode.id, input)
      editorMode = null
      await reload()
      notify(messages.library.saved, 'success')
    } catch (e) {
      const msg = e instanceof Error && e.message ? e.message : String(e)
      notify(`${messages.toast.saveFailed}: ${msg}`, 'error')
    }
  }

  async function handleRemoveAiTag(id: string, normalizedTag: string) {
    try {
      await vaultApi.removeAiTag(id, normalizedTag)
      await reload()
    } catch (e) {
      const msg = e instanceof Error && e.message ? e.message : String(e)
      notify(`${messages.library.removeTagFailed}: ${msg}`, 'error')
    }
  }

  async function handleCopy(payload: { label: string; value: string; sensitive: boolean }) {
    try {
      // Task 18: 所有资料库复制都走 Tauri 命令，敏感值由后端按设置自动清除。
      await vaultApi.copyText(payload.value, payload.sensitive)
      notify(messages.library.copiedLabel.replace('{label}', payload.label), 'success')
    } catch {
      notify(`${messages.toast.copyFailed}: ${payload.label}`, 'error')
    }
  }

  function handleDelete(id: string) {
    // 在 requestDelete 前 sync 当前 allEntries snapshot，让 controller
    // 能找到 originalIndex 用于后续 undo / commit-fail 恢复。
    ctrl.syncAllEntries(allEntries)
    ctrl.requestDelete(id)
    // 立即 bump pendingVersion，触发 derived 重算。
    bumpPendingVersion()
  }

  function handleNewMenuKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault()
      closeNewMenu()
    }
  }

  // ---- lifecycle ----

  onMount(() => {
    let disposed = false
    const unlisteners: UnlistenFn[] = []

    void reload()
    void loadAiSettings()

    const promises: Promise<UnlistenFn>[] = [
      onTagsUpdated(() => { void reload() }),
      onLlmError((e) => {
        notify(`${messages.library.aiError}: ${e.kind} - ${e.code}`, 'error')
      }),
    ]

    // AI metadata / backfill progress events. These events don't yet have
    // typed wrappers in vault.ts; listen directly. The payload shape is
    // not required here — we only trigger a reload.
    void listen('vault-ai-metadata-updated', () => { void reload() }).then((un) => {
      if (disposed) un()
      else unlisteners.push(un)
    })
    void listen('vault-ai-backfill-progress', () => { void reload() }).then((un) => {
      if (disposed) un()
      else unlisteners.push(un)
    })

    void Promise.all(promises).then((items) => {
      if (disposed) items.forEach((un) => un())
      else unlisteners.push(...items)
    })

    return () => {
      disposed = true
      unlisteners.forEach((un) => un())
      searchController.dispose()
      ctrl.dispose()
    }
  })
</script>

<svelte:window onkeydown={(e) => { if (showNewMenu && e.key === 'Escape') closeNewMenu() }} />

<div class="library-view">
  <div class="library-header">
    <div class="header-row header-row-primary">
      <div class="search-wrap">
        <LibrarySearchInput
          controller={searchController}
          autoHybridSearch={autoHybridSearch}
          searchState={hybridState}
          onStartedChange={onSearchStarted}
          onQueryChange={onSearchQueryChange}
        />
      </div>
      <div class="new-menu" data-new-menu>
        <button
          type="button"
          class="new-btn"
          aria-haspopup="menu"
          aria-expanded={showNewMenu}
          onclick={() => (showNewMenu ? closeNewMenu() : openNewMenu())}
        >+ {messages.library.create}</button>
        {#if showNewMenu}
          <!-- I4: 透明全屏 backdrop 拦截外部点击，关闭菜单。
               backdrop 在 popover 之下（z-index 较低），点击它关闭；
               点击 popover 内部不传播到 backdrop。 -->
          <button
            type="button"
            class="new-menu-backdrop"
            aria-label={messages.library.create}
            tabindex="-1"
            onclick={closeNewMenu}
          ></button>
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div
            class="new-menu-popover"
            role="menu"
            aria-label={messages.library.create}
            tabindex="-1"
            onkeydown={handleNewMenuKeydown}
          >
            <button
              type="button"
              role="menuitem"
              class="new-menu-item"
              onclick={() => startCreate('credential')}
            >{messages.library.credential}</button>
            <button
              type="button"
              role="menuitem"
              class="new-menu-item"
              onclick={() => startCreate('bookmark')}
            >{messages.library.bookmark}</button>
            <button
              type="button"
              role="menuitem"
              class="new-menu-item"
              onclick={() => startCreate('note')}
            >{messages.library.note}</button>
          </div>
        {/if}
      </div>
    </div>

    <div class="header-row header-row-filters" role="tablist" aria-label={messages.library.title}>
      <button
        type="button"
        role="tab"
        class="filter-btn"
        class:active={activeFilter === 'all'}
        aria-selected={activeFilter === 'all'}
        onclick={() => selectFilter('all')}
      >{messages.library.all} {counts.all}</button>
      <button
        type="button"
        role="tab"
        class="filter-btn"
        class:active={activeFilter === 'credential'}
        aria-selected={activeFilter === 'credential'}
        onclick={() => selectFilter('credential')}
      >{messages.library.credential} {counts.credential}</button>
      <button
        type="button"
        role="tab"
        class="filter-btn"
        class:active={activeFilter === 'bookmark'}
        aria-selected={activeFilter === 'bookmark'}
        onclick={() => selectFilter('bookmark')}
      >{messages.library.bookmark} {counts.bookmark}</button>
      <button
        type="button"
        role="tab"
        class="filter-btn"
        class:active={activeFilter === 'note'}
        aria-selected={activeFilter === 'note'}
        onclick={() => selectFilter('note')}
      >{messages.library.note} {counts.note}</button>
    </div>
  </div>

  {#if editorMode}
    <div class="editor-panel">
      {#if editorMode.mode === 'create'}
        <VaultEntryEditor
          mode="create"
          initialKind={editorMode.kind}
          onSave={handleSaveCreate}
          onCancel={cancelEditor}
        />
      {:else}
        {#key editorMode.id}
          <VaultEntryEditor
            mode="edit"
            initial={editorMode.detail}
            onSave={handleSaveEdit}
            onCancel={cancelEditor}
            onRemoveAiTag={handleRemoveAiTag}
          />
        {/key}
      {/if}
    </div>
  {/if}

  <div class="library-body">
    {#if emptyState.kind === 'loading'}
      <div class="dockEmpty" aria-live="polite">
        <div>{messages.settings.checking}</div>
      </div>
    {:else if emptyState.kind === 'no-results'}
      <div class="dockEmpty" aria-live="polite">
        <div>{messages.library.noMatch}</div>
        <div class="hint">{messages.library.searchPlaceholder}</div>
      </div>
    {:else if emptyState.kind === 'empty'}
      <div class="dockEmpty" aria-live="polite">
        <div>{messages.library.empty}</div>
        <div class="hint">+ {messages.library.create}</div>
      </div>
    {:else}
      <div class="entry-list">
        {#each visibleSummaries as e (e.entry.id)}
          <EntryCard
            summary={e}
            onLoadDetail={(id) => vaultApi.getEntry(id)}
            onCopy={handleCopy}
            onEdit={(id) => { void startEdit(id) }}
            onDelete={(id) => handleDelete(id)}
            onRemoveAiTag={handleRemoveAiTag}
          />
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .library-view {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.5rem 0.65rem;
    overflow: hidden;
    min-height: 0;
  }

  .library-header {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    flex-shrink: 0;
  }

  .header-row {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  .header-row-primary {
    gap: 0.3rem;
  }

  .search-wrap {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .new-menu {
    position: relative;
    flex-shrink: 0;
  }

  .new-btn {
    background: color-mix(in srgb, var(--color-primary) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    color: var(--color-primary);
    font-size: var(--font-sm, 0.65rem);
    font-weight: 500;
    padding: 0.25rem 0.55rem;
    border-radius: var(--radius-md, 0.3rem);
    cursor: pointer;
    font-family: inherit;
    white-space: nowrap;
    transition: background 0.12s, border-color 0.12s;
  }

  .new-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 22%, transparent);
  }

  .new-menu-popover {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 0.2rem;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 0.3rem);
    padding: 0.25rem;
    min-width: 5rem;
    box-shadow: var(--shadow-default);
    z-index: 60;
    outline: none;
  }

  /* I4: 透明全屏 backdrop，捕获 popover 外部的点击以关闭菜单。 */
  .new-menu-backdrop {
    position: fixed;
    inset: 0;
    background: transparent;
    border: none;
    padding: 0;
    margin: 0;
    cursor: default;
    z-index: 55;
  }

  .new-menu-item {
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: var(--font-sm, 0.7rem);
    padding: 0.3rem 0.45rem;
    border-radius: var(--radius-md, 0.25rem);
    cursor: pointer;
    font-family: inherit;
    text-align: left;
    transition: background 0.12s, color 0.12s;
  }

  .new-menu-item:hover,
  .new-menu-item:focus-visible {
    background: color-mix(in srgb, var(--color-primary) 14%, transparent);
    color: var(--color-primary);
    outline: none;
  }

  .header-row-filters {
    gap: 0.15rem;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .header-row-filters::-webkit-scrollbar {
    display: none;
  }

  .filter-btn {
    background: none;
    border: 1px solid transparent;
    color: color-mix(in srgb, var(--text-primary) 50%, transparent);
    font-size: var(--font-sm, 0.65rem);
    font-weight: 500;
    padding: 0.2rem 0.45rem;
    border-radius: var(--radius-md, 0.3rem);
    cursor: pointer;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
    font-family: inherit;
    white-space: nowrap;
  }

  .filter-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
    border-color: color-mix(in srgb, var(--text-primary) 15%, transparent);
  }

  .filter-btn.active {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 15%, transparent);
    border-color: color-mix(in srgb, var(--text-primary) 25%, transparent);
  }

  .editor-panel {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.5rem;
    background: var(--surface-1);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg, 0.5rem);
    flex-shrink: 0;
    max-height: 60%;
    overflow-y: auto;
  }

  .library-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .dockEmpty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 2rem 0.5rem;
    color: var(--text-faint);
    font-size: var(--font-sm, 0.75rem);
    text-align: center;
    gap: 0.2rem;
  }

  .dockEmpty .hint {
    font-size: var(--font-sm, 0.65rem);
    color: var(--text-faint);
    opacity: 0.7;
  }

  .entry-list {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
</style>
