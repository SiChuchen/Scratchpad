<script lang="ts">
  import type { DockEntry, EntryKind } from '$lib/types/dock'
  import EntryCard from '$lib/components/EntryCard.svelte'

  interface Props {
    entries: DockEntry[]
    onToggleCollapse: (entryId: string) => void
    onDeleteFromView: (entryId: string) => void
    onAddToNote: (entryId: string) => void
    onUpdateText: (id: string, content: string) => void
    onCopy: (content: string) => void
    onCopyPath: (path: string) => void
  }

  let {
    entries,
    onToggleCollapse,
    onDeleteFromView,
    onAddToNote,
    onUpdateText,
    onCopy,
    onCopyPath,
  }: Props = $props()

  let activeFilter = $state<EntryKind | null>(null)

  let filtered = $derived(
    activeFilter
      ? entries.filter((e) => e.kind === activeFilter)
      : entries,
  )

  let filters: { kind: EntryKind | null; label: string }[] = [
    { kind: null, label: '全部' },
    { kind: 'text', label: '文本' },
    { kind: 'image', label: '图片' },
    { kind: 'file', label: '文件' },
  ]
</script>

<div class="categories-view">
  <div class="filter-bar">
    {#each filters as f}
      <button
        class="filter-btn"
        class:active={activeFilter === f.kind}
        onclick={() => (activeFilter = f.kind)}
      >{f.label}</button>
    {/each}
  </div>

  <div class="categories-body">
    {#if filtered.length === 0}
      <div class="dock-empty">
        <p>{activeFilter ? '该分类暂无内容' : '暂无内容'}</p>
      </div>
    {:else}
      <div class="entry-list">
        {#each filtered as entry (entry.id)}
          <EntryCard
            {entry}
            {onToggleCollapse}
            {onDeleteFromView}
            {onAddToNote}
            {onUpdateText}
            {onCopy}
            {onCopyPath}
          />
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .categories-view {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.5rem 0.65rem;
    overflow: hidden;
    min-height: 0;
  }

  .filter-bar {
    display: flex;
    align-items: center;
    gap: 0.15rem;
    flex-shrink: 0;
    height: 1.75rem;
  }

  .filter-btn {
    background: none;
    border: 1px solid rgba(148, 163, 184, 0.12);
    color: rgba(148, 163, 184, 0.5);
    font-size: 0.6rem;
    padding: 0.25rem 0.55rem;
    border-radius: 0.3rem;
    cursor: pointer;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
    font-family: inherit;
    height: 1.75rem;
    display: flex;
    align-items: center;
  }

  .filter-btn:hover {
    color: #e5eef7;
    background: rgba(148, 163, 184, 0.08);
  }

  .filter-btn.active {
    color: rgba(125, 211, 252, 0.9);
    border-color: rgba(125, 211, 252, 0.3);
    background: rgba(125, 211, 252, 0.1);
  }

  .categories-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .dock-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 2rem 0;
    color: rgba(148, 163, 184, 0.4);
    font-size: 0.75rem;
  }

  .entry-list {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
</style>
