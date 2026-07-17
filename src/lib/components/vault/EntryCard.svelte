<script lang="ts">
  // src/lib/components/vault/EntryCard.svelte
  //
  // 窄窗口下 Vault 条目的折叠卡片。点击 header（或键盘 Enter/Space）后
  // 调用 onLoadDetail 取得完整 detail 并内联展开 VaultEntryDetail。
  //
  // 关键 UX 要点：
  //   * 折叠状态下只显示 kind / title / preview / tags (chips, 最多 3 个)；
  //   * 切换条目或折叠时递增 resetToken，强制 CopyableValue 重新掩码；
  //   * 编辑按钮调 onEdit；删除按钮调 onDelete，绝不用 confirm()。

  import type {
    VaultEntryDetail as VaultEntryDetailType,
    VaultEntrySummary,
  } from '$lib/types/vault'
  import { messages } from '$lib/i18n'
  import VaultEntryDetail from './VaultEntryDetail.svelte'

  interface Props {
    summary: VaultEntrySummary
    resetToken?: number | string
    onLoadDetail?: (id: string) => Promise<VaultEntryDetailType>
    onCopy: (payload: {
      label: string
      value: string
      sensitive: boolean
    }) => void | Promise<void>
    onEdit?: (id: string) => void | Promise<void>
    onDelete?: (id: string) => void | Promise<void>
    onRemoveAiTag?: (id: string, normalizedTag: string) => Promise<void>
  }

  let {
    summary,
    resetToken = 0,
    onLoadDetail,
    onCopy,
    onEdit,
    onDelete,
    onRemoveAiTag,
  }: Props = $props()

  let expanded = $state(false)
  let detail = $state<VaultEntryDetailType | null>(null)
  let loading = $state(false)
  let error = $state<string | null>(null)
  // 本地 resetToken：在 expand/collapse/entry 切换时递增以强制掩码
  let localReset = $state(0)
  // 跟踪已加载的 entryId，避免重复加载 / 切换时残留
  let loadedId = $state<string | null>(null)

  const kindLabel = $derived(
    summary.entry.kind === 'credential'
      ? messages.library.credential
      : summary.entry.kind === 'bookmark'
        ? messages.library.bookmark
        : messages.library.note,
  )

  const tags = $derived(summary.tags.slice(0, 3))

  async function toggleExpand() {
    if (expanded) {
      expanded = false
      localReset += 1
      return
    }
    if (!onLoadDetail) {
      expanded = true
      return
    }
    if (loadedId !== summary.entry.id || !detail) {
      loading = true
      error = null
      try {
        detail = await onLoadDetail(summary.entry.id)
        loadedId = summary.entry.id
      } catch (e) {
        error = e instanceof Error ? e.message : String(e)
      } finally {
        loading = false
      }
    }
    expanded = true
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      void toggleExpand()
    }
  }

  function handleRemoveAiTag(normalizedTag: string) {
    if (!onRemoveAiTag) return
    return onRemoveAiTag(summary.entry.id, normalizedTag)
  }

  // 切换条目时清空本地 detail（让下次 expand 重新加载）+ 递增 reset
  $effect(() => {
    void summary.entry.id
    if (loadedId !== null && loadedId !== summary.entry.id) {
      detail = null
      loadedId = null
      expanded = false
      localReset += 1
    }
  })

  const combinedReset = $derived(`${resetToken}:${localReset}`)
</script>

<div class="entry-card" data-expanded={expanded}>
  <div
    class="entry-header"
    role="button"
    tabindex="0"
    aria-expanded={expanded}
    aria-label="{kindLabel} {summary.entry.title}"
    onclick={() => void toggleExpand()}
    onkeydown={onKeydown}
  >
    <span class="kind-badge">{kindLabel}</span>
    <span class="title">{summary.entry.title}</span>
    <div class="spacer"></div>
    <div class="header-actions">
      {#if onEdit}
        <button
          type="button"
          class="icon-btn"
          aria-label={`${messages.library.edit} ${summary.entry.title}`}
          title={messages.library.edit}
          onclick={(e) => {
            e.stopPropagation()
            void onEdit(summary.entry.id)
          }}
        >
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"></path>
            <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"></path>
          </svg>
        </button>
      {/if}
      {#if onDelete}
        <button
          type="button"
          class="icon-btn danger"
          aria-label={`${messages.library.delete} ${summary.entry.title}`}
          title={messages.library.delete}
          onclick={(e) => {
            e.stopPropagation()
            void onDelete(summary.entry.id)
          }}
        >
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <polyline points="3 6 5 6 21 6"></polyline>
            <path d="M19 6l-2 14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2L5 6"></path>
          </svg>
        </button>
      {/if}
      <span class="chevron" aria-hidden="true">{expanded ? '▾' : '▸'}</span>
    </div>
  </div>

  {#if !expanded && (summary.preview || tags.length > 0)}
    <div class="preview-row">
      {#if summary.preview}
        <span class="preview">{summary.preview}</span>
      {/if}
      {#if tags.length > 0}
        <div class="tag-chips">
          {#each tags as t (t.normalizedTag)}
            <span class="tag-chip {t.source}">{t.tag}</span>
          {/each}
          {#if summary.tags.length > 3}
            <span class="tag-more">+{summary.tags.length - 3}</span>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if expanded}
    <div class="detail-wrap">
      {#if loading}
        <div class="loading">{messages.settings.checking}</div>
      {:else if error}
        <div class="error">{messages.toast.loadFailed}: {error}</div>
      {:else if detail}
        <VaultEntryDetail
          {detail}
          resetToken={combinedReset}
          {onCopy}
          onRemoveAiTag={onRemoveAiTag ? handleRemoveAiTag : undefined}
        />
      {/if}
    </div>
  {/if}
</div>

<style>
  .entry-card {
    background: var(--surface-1);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg, 0.5rem);
    padding: 0.4rem 0.55rem;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .entry-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    cursor: pointer;
    border-radius: var(--radius-md, 0.25rem);
    padding: 0.1rem 0;
    outline: none;
  }

  .entry-header:focus-visible {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-primary) 40%, transparent);
  }

  .kind-badge {
    font-size: 0.55rem;
    color: var(--text-muted);
    padding: 0.1rem 0.35rem;
    background: var(--surface-2);
    border-radius: var(--radius-md, 0.2rem);
    flex-shrink: 0;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .title {
    font-size: var(--font-sm, 0.75rem);
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .spacer {
    flex: 1;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 0.15rem;
    flex-shrink: 0;
  }

  .icon-btn {
    background: none;
    border: 1px solid transparent;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0.2rem;
    border-radius: var(--radius-md, 0.2rem);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
  }

  .icon-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
  }

  .icon-btn.danger:hover {
    color: #ff6b6b;
    background: color-mix(in srgb, #ff6b6b 15%, transparent);
  }

  .chevron {
    font-size: 0.65rem;
    color: var(--text-muted);
    padding: 0 0.15rem;
  }

  .preview-row {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    padding-left: 0.1rem;
  }

  .preview {
    font-size: var(--font-sm, 0.65rem);
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tag-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem;
    align-items: center;
  }

  .tag-chip {
    font-size: 0.55rem;
    padding: 0.05rem 0.3rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 0.2rem);
    color: var(--text-muted);
  }

  .tag-chip.ai {
    color: var(--color-primary, #6c8cff);
    border-color: color-mix(in srgb, var(--color-primary, #6c8cff) 25%, transparent);
    background: color-mix(in srgb, var(--color-primary, #6c8cff) 8%, transparent);
  }

  .tag-more {
    font-size: 0.55rem;
    color: var(--text-faint, var(--text-muted));
  }

  .detail-wrap {
    padding-top: 0.1rem;
  }

  .loading,
  .error {
    font-size: var(--font-sm, 0.65rem);
    color: var(--text-muted);
    padding: 0.2rem 0.1rem;
  }

  .error {
    color: #ff6b6b;
  }
</style>
