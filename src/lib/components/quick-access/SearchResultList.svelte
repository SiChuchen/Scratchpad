<script lang="ts">
  // src/lib/components/quick-access/SearchResultList.svelte
  //
  // SearchMode 左栏列表：渲染 VaultSearchHit[]。每行展示 kind、title、来源
  // 徽标（Local / AI）以及最多 3 个 tag。键盘上下键由父组件 SearchMode 消费，
  // 这里只负责点击选中 + 把当前选中项滚动进入视口。
  //
  // a11y：每行使用 role="option" + aria-selected；外层 listbox 由父组件提供。

  import type { VaultSearchHit } from '$lib/types/vault'

  interface Props {
    hits: VaultSearchHit[]
    selectedId: string | null
    onSelect: (id: string) => void
  }

  let { hits, selectedId, onSelect }: Props = $props()

  const kindLabel = (kind: string) =>
    kind === 'credential' ? '凭据' : kind === 'bookmark' ? '书签' : '笔记'

  const sourceLabel = (sources: VaultSearchHit['sources']) =>
    sources.includes('aiExpanded') ? 'AI' : 'Local'

  // Scroll selected option into view.
  let listEl: HTMLDivElement | null = $state(null)
  $effect(() => {
    void selectedId
    void hits
    if (!listEl) return
    const sel = listEl.querySelector<HTMLElement>('[aria-selected="true"]')
    // jsdom doesn't implement scrollIntoView; guard.
    if (sel && typeof sel.scrollIntoView === 'function') {
      sel.scrollIntoView({ block: 'nearest' })
    }
  })
</script>

<div class="search-result-list" bind:this={listEl} role="listbox" aria-label="搜索结果">
  {#each hits as hit (hit.summary.entry.id)}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div
      class="result-row"
      class:selected={hit.summary.entry.id === selectedId}
      role="option"
      aria-selected={hit.summary.entry.id === selectedId}
      tabindex="-1"
      onclick={() => onSelect(hit.summary.entry.id)}
    >
      <span class="kind-badge">{kindLabel(hit.summary.entry.kind)}</span>
      <span class="title">{hit.summary.entry.title}</span>
      <span class="source-badge" class:ai={sourceLabel(hit.sources) === 'AI'}>
        {sourceLabel(hit.sources)}
      </span>
      {#if hit.summary.tags.length > 0}
        <div class="tag-chips">
          {#each hit.summary.tags.slice(0, 3) as t (t.normalizedTag)}
            <span class="tag-chip {t.source}">{t.tag}</span>
          {/each}
          {#if hit.summary.tags.length > 3}
            <span class="tag-more">+{hit.summary.tags.length - 3}</span>
          {/if}
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .search-result-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-height: 0;
  }

  .result-row {
    display: grid;
    grid-template-columns: auto 1fr auto;
    grid-template-rows: auto auto;
    align-items: center;
    gap: 0.25rem 0.4rem;
    padding: 0.3rem 0.4rem;
    border-radius: var(--radius-md, 6px);
    cursor: pointer;
    border: 1px solid transparent;
  }

  .result-row:hover {
    background: var(--surface-2);
  }

  .result-row.selected {
    background: color-mix(in srgb, var(--color-primary, #4f46e5) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-primary, #4f46e5) 40%, transparent);
  }

  .kind-badge {
    font-size: 0.55rem;
    color: var(--text-muted);
    padding: 0.1rem 0.3rem;
    background: var(--surface-2);
    border-radius: var(--radius-md, 4px);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .title {
    font-size: var(--font-sm, 13px);
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .source-badge {
    font-size: 0.55rem;
    padding: 0.05rem 0.3rem;
    border-radius: var(--radius-md, 4px);
    background: var(--surface-2);
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .source-badge.ai {
    color: var(--color-primary, #6c8cff);
    background: color-mix(in srgb, var(--color-primary, #6c8cff) 12%, transparent);
  }

  .tag-chips {
    grid-column: 1 / -1;
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
    border-radius: var(--radius-md, 4px);
    color: var(--text-muted);
  }

  .tag-chip.ai {
    color: var(--color-primary, #6c8cff);
    border-color: color-mix(in srgb, var(--color-primary, #6c8cff) 25%, transparent);
    background: color-mix(in srgb, var(--color-primary, #6c8cff) 8%, transparent);
  }

  .tag-more {
    font-size: 0.55rem;
    color: var(--text-muted);
  }
</style>
