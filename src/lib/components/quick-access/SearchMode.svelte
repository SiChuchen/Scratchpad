<script lang="ts">
  // src/lib/components/quick-access/SearchMode.svelte
  //
  // 全局"搜索"双栏模式：左栏列表 + 右栏详情。
  //
  // 时序（与 Task 17 spec 对齐）：
  //   * query 输入 → 300ms 防抖 → HybridSearchController.search() 或
  //     searchLocalOnly()（依据 autoHybridSearch）。
  //   * HybridSearchController 通过 onState 回调驱动 hits / selectedId / phase /
  //     understoodTerms / error。
  //   * selectedId 变化时通过 revision guard 调 getEntry() 加载右栏 detail。
  //   * ArrowDown/ArrowUp 在搜索框聚焦时改变 selectedId；Tab 仍能进入右栏
  //     复制按钮；Enter 不做特殊处理（保持简单）。
  //   * resetToken（来自 QuickAccessApp blur handler）变化时递增 localResetToken，
  //     传递给 VaultEntryDetail → CopyableValue 强制重新掩码。
  //
  // 复制：调用 notify(字段名) 不关闭面板；面板始终显示。

  import { onMount, onDestroy } from 'svelte'
  import { vaultApi } from '$lib/api/vault'
  import {
    HybridSearchController,
    type HybridSearchApi,
    type HybridSearchState,
  } from '$lib/state/vault-search'
  import type { VaultEntryDetail as VaultEntryDetailType } from '$lib/types/vault'
  import VaultEntryDetail from '$lib/components/vault/VaultEntryDetail.svelte'
  import SearchResultList from './SearchResultList.svelte'

  interface Props {
    notify: (
      text: string,
      kind?: 'success' | 'error',
      undo?: () => void,
      actionLabel?: string,
    ) => void
    resetToken?: number | string
    autoHybridSearch?: boolean
  }

  let {
    notify,
    resetToken = 0,
    autoHybridSearch = false,
  }: Props = $props()

  const SEARCH_DEBOUNCE_MS = 300

  // ---- State --------------------------------------------------------------

  let query = $state('')
  let hits = $state<HybridSearchState['hits']>([])
  let selectedId = $state<string | null>(null)
  let understoodTerms = $state<string[]>([])
  let phase = $state<HybridSearchState['phase']>('idle')
  let errorMessage = $state<string | null>(null)

  let selectedDetail = $state<VaultEntryDetailType | null>(null)
  let detailLoading = $state(false)
  let detailError = $state<string | null>(null)
  let detailRevision = $state(0)

  // Local resetToken mirrors parent prop changes; bumps when parent resets.
  let localResetToken = $state(0)
  let lastReset: number | string = 0
  let initializedReset = false
  $effect(() => {
    // Read resetToken inside the effect so we detect every change.
    const next = resetToken
    // Skip the very first run; only bump on subsequent changes.
    if (initializedReset) {
      if (next !== lastReset) {
        lastReset = next
        localResetToken += 1
      }
    } else {
      lastReset = next
      initializedReset = true
    }
  })

  // Combined reset for VaultEntryDetail (also bumps on selectedId change so the
  // newly loaded detail re-masks).
  const detailReset = $derived(`${localResetToken}:${selectedId}`)

  // ---- Controller ---------------------------------------------------------

  let controller: HybridSearchController | null = null
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  let lastQuery = ''

  onMount(() => {
    const api: HybridSearchApi = {
      searchLocal: (q, plan, limit) => vaultApi.searchLocal(q, plan, limit),
      planSearch: (q, rid) => vaultApi.planSearch(q, rid),
      cancelSearch: (rid) => vaultApi.cancelSearch(rid),
    }
    controller = new HybridSearchController({
      api,
      onState: onSearchState,
    })
  })

  onDestroy(() => {
    if (debounceTimer) {
      clearTimeout(debounceTimer)
      debounceTimer = null
    }
    controller?.dispose()
    controller = null
  })

  function onSearchState(state: HybridSearchState) {
    hits = state.hits
    // Preserve user-clicked selection if controller's selectedId differs.
    // The controller already implements "preserve or fall back to first".
    selectedId = state.selectedId
    understoodTerms = state.understoodTerms
    phase = state.phase
    errorMessage = state.error
  }

  // ---- Input handling -----------------------------------------------------

  function onQueryInput(e: Event) {
    const v = (e.currentTarget as HTMLInputElement).value
    query = v
    if (debounceTimer) {
      clearTimeout(debounceTimer)
      debounceTimer = null
    }
    if (!v.trim()) {
      // Clear results when input is emptied.
      hits = []
      selectedId = null
      understoodTerms = []
      phase = 'idle'
      errorMessage = null
      lastQuery = ''
      return
    }
    debounceTimer = setTimeout(() => {
      debounceTimer = null
      void runSearch(v)
    }, SEARCH_DEBOUNCE_MS)
  }

  function runSearch(q: string) {
    if (!controller) return
    lastQuery = q
    if (autoHybridSearch) {
      void controller.search(q)
    } else {
      void controller.searchLocalOnly(q)
    }
  }

  // ---- Detail loading (revision guard) ------------------------------------

  let loadedDetailId = $state<string | null>(null)
  let inflightDetailId = $state<string | null>(null)

  $effect(() => {
    const id = selectedId
    if (id === null) {
      selectedDetail = null
      detailError = null
      loadedDetailId = null
      inflightDetailId = null
      return
    }
    // Already loaded for this id.
    if (id === loadedDetailId && selectedDetail) return
    // Already in-flight for this id (don't double-fire).
    if (id === inflightDetailId) return
    inflightDetailId = id
    const myRevision = ++detailRevision
    detailLoading = true
    detailError = null
    // Wrap in Promise.resolve so we tolerate `getEntry` returning undefined
    // (e.g. during tests where the mock hasn't been configured).
    Promise.resolve(vaultApi.getEntry(id))
      .then((d) => {
        if (myRevision !== detailRevision) return
        selectedDetail = d
        loadedDetailId = id
      })
      .catch((e: unknown) => {
        if (myRevision !== detailRevision) return
        detailError = e instanceof Error ? e.message : String(e)
        selectedDetail = null
      })
      .finally(() => {
        if (myRevision !== detailRevision) return
        detailLoading = false
      })
  })

  // ---- Keyboard -----------------------------------------------------------

  function onKeydown(e: KeyboardEvent) {
    if (hits.length === 0) return
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const idx = hits.findIndex((h) => h.summary.entry.id === selectedId)
      let nextIdx: number
      if (e.key === 'ArrowDown') {
        nextIdx = idx < 0 ? 0 : Math.min(idx + 1, hits.length - 1)
      } else {
        nextIdx = idx <= 0 ? 0 : idx - 1
      }
      const nextId = hits[nextIdx]?.summary.entry.id ?? null
      if (nextId && nextId !== selectedId) {
        selectedId = nextId
        controller?.setSelectedId(nextId)
      }
    }
  }

  // ---- Selection / copy ---------------------------------------------------

  function onSelect(id: string) {
    selectedId = id
    controller?.setSelectedId(id)
  }

  async function handleCopy(payload: {
    label: string
    value: string
    sensitive: boolean
  }) {
    try {
      await navigator.clipboard.writeText(payload.value)
      notify(`已复制：${payload.label}`, 'success')
    } catch {
      notify(`复制失败：${payload.label}`, 'error')
    }
  }

  // ---- Derived view -------------------------------------------------------

  const statusText = $derived.by(() => {
    if (phase === 'planning') return 'AI 理解中…'
    if (phase === 'expanded' && understoodTerms.length > 0) {
      return `AI 已理解：${understoodTerms.join('、')}`
    }
    if (phase === 'error') return errorMessage ?? '搜索失败'
    return ''
  })
</script>

<section class="mode mode-search">
  <header class="mode-header">
    <h2>搜索</h2>
    <span class="hint">Ctrl+Tab → 录入</span>
  </header>

  <input
    class="search-input"
    type="search"
    placeholder="搜索资料库…"
    value={query}
    oninput={onQueryInput}
    onkeydown={onKeydown}
  />

  {#if statusText}
    <div class="status" class:error={phase === 'error'}>{statusText}</div>
  {/if}

  <div class="dual-pane">
    <div class="left-pane">
      {#if hits.length === 0}
        <div class="empty-list">
          {#if query}
            <p class="muted">没有匹配的资料</p>
          {:else}
            <p class="muted">输入关键词以开始搜索</p>
          {/if}
        </div>
      {:else}
        <SearchResultList {hits} {selectedId} onSelect={onSelect} />
      {/if}
    </div>

    <div class="right-pane">
      {#if detailLoading}
        <div class="detail-state">加载中…</div>
      {:else if detailError}
        <div class="detail-state error">加载失败：{detailError}</div>
      {:else if selectedDetail}
        <VaultEntryDetail
          detail={selectedDetail}
          resetToken={detailReset}
          onCopy={handleCopy}
        />
      {:else}
        <div class="detail-state">选择一条资料查看详情</div>
      {/if}
    </div>
  </div>
</section>

<style>
  .mode-search {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.75rem;
    min-height: 0;
  }

  .mode-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .mode-header h2 {
    margin: 0;
    font-size: var(--font-md, 15px);
    font-weight: 600;
    color: var(--text-primary);
  }
  .hint {
    font-size: var(--font-xs, 11px);
    color: var(--text-muted);
  }

  .search-input {
    width: 100%;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 6px);
    padding: 0.5rem 0.65rem;
    background: var(--surface-1);
    color: var(--text-primary);
    font-family: var(--font-family-en, 'Segoe UI'),
      var(--font-family-zh, 'Microsoft YaHei'), sans-serif;
    font-size: var(--font-md, 15px);
    outline: none;
  }
  .search-input:focus {
    border-color: var(--color-primary);
  }

  .status {
    font-size: var(--font-xs, 11px);
    color: var(--color-primary, #4f46e5);
    padding: 0.25rem 0.4rem;
    border-radius: var(--radius-md, 6px);
    background: color-mix(in srgb, var(--color-primary, #4f46e5) 8%, transparent);
  }
  .status.error {
    color: var(--color-danger, #ef4444);
    background: color-mix(in srgb, var(--color-danger, #ef4444) 8%, transparent);
  }

  .dual-pane {
    flex: 1;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 0.5rem;
    min-height: 0;
  }

  .left-pane,
  .right-pane {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 6px);
    background: var(--surface-1);
    padding: 0.35rem;
  }

  .empty-list {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .muted {
    color: var(--text-muted);
    font-size: var(--font-sm, 13px);
    margin: 0;
  }

  .detail-state {
    color: var(--text-muted);
    font-size: var(--font-sm, 13px);
    padding: 0.5rem;
  }
  .detail-state.error {
    color: var(--color-danger, #ef4444);
  }
</style>
