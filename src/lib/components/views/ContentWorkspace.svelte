<script lang="ts">
  import { tick } from 'svelte'
  import ContentSearchBar from '$lib/components/content/ContentSearchBar.svelte'
  import ContentList from '$lib/components/content/ContentList.svelte'
  import ContentDetailView from '$lib/components/content/ContentDetail.svelte'
  import Icon from '$lib/components/Icon.svelte'
  import { messages } from '$lib/i18n'
  import type { ContentBrowserState } from '$lib/state/content-browser'
  import type { ContentSearchState } from '$lib/state/content-search'
  import type { BrowseScope, ContentDetail, ContentKind, ContentSummary } from '$lib/types/content'

  interface Props {
    browser: ContentBrowserState
    search: ContentSearchState
    selectedDetail: ContentDetail | null
    detailLoading: boolean
    pendingDeleteIds?: string[]
    onSearch: (q: string) => void
    onClearSearch: () => void
    onSelect: (id: string | null) => void
    onSetKind: (kind: ContentKind | null) => void
    onReorder: (ids: string[]) => Promise<void>
    onToggleSaved: (item: ContentSummary) => void
    onCopy: (item: ContentSummary) => void
    onDelete: (item: ContentSummary) => void
    onCreateText: () => void
    onDetailChanged: (id: string) => Promise<void>
    onNotify: (m: string, k?: 'success' | 'error') => void
  }

  let {
    browser,
    search,
    selectedDetail,
    detailLoading,
    pendingDeleteIds = [],
    onSearch,
    onClearSearch,
    onSelect,
    onSetKind,
    onReorder,
    onToggleSaved,
    onCopy,
    onDelete,
    onCreateText,
    onDetailChanged,
    onNotify,
  }: Props = $props()

  let scroll: HTMLDivElement
  let wasSearching = $state(false)
  let preSearchSelection = $state<string | null>(null)
  let scopeScrollTop = $state<Record<BrowseScope, number>>({ temporary: 0, all: 0, saved: 0 })

  const searching = $derived(search.query.trim().length > 0)
  const items = $derived(searching ? search.hits.map((h) => h.summary) : browser.items)
  const selectedId = $derived(searching ? search.selectedId : browser.selectedId)
  const reorderable = $derived(!searching && browser.scope !== 'all')

  const contextLabel = $derived(
    searching
      ? messages.workspace.resultsFound.replace('{n}', String(items.length))
      : browser.scope === 'temporary'
        ? messages.workspace.temporaryRetention
        : messages.workspace.itemsCount.replace('{n}', String(items.length)),
  )

  const emptyTitle = $derived(
    searching
      ? messages.workspace.noResults
      : browser.scope === 'temporary'
        ? messages.workspace.emptyInbox
        : browser.scope === 'saved'
          ? messages.workspace.emptySaved
          : messages.workspace.emptyAll,
  )
  const emptyHint = $derived(
    searching ? messages.workspace.emptySearchHint : messages.workspace.emptyInboxHint,
  )

  function searchInput(q: string) {
    if (!wasSearching && q.trim()) {
      scopeScrollTop[browser.scope] = scroll?.scrollTop ?? 0
      preSearchSelection = browser.selectedId
      wasSearching = true
    }
    onSearch(q)
  }

  async function clear() {
    onClearSearch()
    wasSearching = false
    await tick()
    if (preSearchSelection) onSelect(preSearchSelection)
    if (scroll) scroll.scrollTop = scopeScrollTop[browser.scope]
  }

  function select(id: string | null) {
    onSelect(id)
  }
</script>

<div class="workspace">
  <ContentSearchBar
    query={search.query}
    selectedKind={browser.kind}
    searching={search.phase === 'searching'}
    onSearch={searchInput}
    onClear={clear}
    {onSetKind}
  />
  <div class="context">
    <span class="context-label">{contextLabel}</span>
    {#if browser.scope === 'temporary' && !searching}
      <button type="button" class="create-btn" onclick={onCreateText}>＋ {messages.workspace.createText}</button>
      <span class="hint">{messages.workspace.pasteHint}</span>
    {/if}
  </div>
  <div class="body" class:detail-open={!!selectedDetail || detailLoading}>
    <div
      class="scroll"
      data-testid="content-scroll"
      bind:this={scroll}
      onscroll={() => {
        if (!searching) scopeScrollTop[browser.scope] = scroll.scrollTop
      }}
    >
      {#if items.length}
        <ContentList {items} {selectedId} {reorderable} busyIds={pendingDeleteIds} onSelect={(id) => select(id)} {onReorder} {onToggleSaved} {onCopy} {onDelete} />
      {:else}
        <div class="empty">
          <span class="empty-icon" aria-hidden="true"><Icon name={searching ? 'search' : 'inbox'} size={26} strokeWidth={1.4} /></span>
          <strong>{emptyTitle}</strong>
          <span>{emptyHint}</span>
        </div>
      {/if}
    </div>
    {#if detailLoading}
      <div class="detail loading" role="status">{messages.workspace.loadingDetail}</div>
    {:else if selectedDetail}
      <div class="detail">
        <ContentDetailView
          detail={selectedDetail}
          resetToken={selectedDetail.summary.id}
          onClose={() => select(null)}
          onChanged={onDetailChanged}
          {onNotify}
          onDelete={() => onDelete(selectedDetail.summary)}
          onToggleSaved={() => onToggleSaved(selectedDetail.summary)}
        />
      </div>
    {/if}
  </div>
</div>

<style>
  .workspace {
    min-height: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .context {
    min-height: 2.1rem;
    display: flex;
    align-items: center;
    gap: 0.55rem;
    padding: 0.3rem 0.65rem;
    color: var(--text-muted);
    font-size: max(var(--font-xs, 0.68rem), 0.68rem);
    border-bottom: 1px solid var(--border-subtle);
  }

  .context-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .context .hint {
    color: var(--text-faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .create-btn {
    margin-inline-start: auto;
    flex: 0 0 auto;
    min-height: 1.9rem;
    padding: 0.15rem 0.6rem;
    border: 1px solid color-mix(in srgb, var(--color-primary) 40%, transparent);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--color-primary) 12%, var(--surface-1));
    color: var(--color-primary);
    font: inherit;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.12s, border-color 0.12s;
  }

  .create-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 20%, var(--surface-1));
    border-color: color-mix(in srgb, var(--color-primary) 55%, transparent);
  }

  .body {
    min-height: 0;
    flex: 1;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    position: relative;
  }

  .scroll {
    min-width: 0;
    overflow: auto;
    padding: 0.5rem;
  }

  .detail {
    position: absolute;
    inset: 0;
    z-index: 10;
    overflow: hidden;
    background: var(--surface-0);
  }

  .loading {
    display: grid;
    place-items: center;
    color: var(--text-muted);
  }

  .empty {
    min-height: 14rem;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.45rem;
    text-align: center;
    color: var(--text-muted);
    font-size: max(var(--font-sm, 0.75rem), 0.75rem);
    padding: 0 1.5rem;
  }

  .empty-icon {
    display: inline-flex;
    padding: 0.8rem;
    border-radius: 50%;
    color: var(--text-faint);
    background: color-mix(in srgb, var(--text-primary) 5%, transparent);
    margin-bottom: 0.25rem;
  }

  .empty strong {
    color: var(--text-primary);
    font-size: max(var(--font-md, 0.85rem), 0.85rem);
  }

  @media (min-width: 680px) {
    .body.detail-open {
      grid-template-columns: minmax(280px, 0.9fr) minmax(340px, 1.1fr);
    }
    .body.detail-open .detail {
      position: static;
      border-left: 1px solid var(--border-subtle);
    }
  }

  @media (max-width: 300px) {
    .context {
      align-items: flex-start;
      flex-wrap: wrap;
    }
    .create-btn {
      margin-inline-start: 0;
    }
    .scroll {
      padding: 0.3rem;
    }
  }
</style>
