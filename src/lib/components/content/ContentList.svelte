<script lang="ts">
  import { tick } from 'svelte'
  import ContentSummaryCard from './ContentSummaryCard.svelte'
  import type { ContentSummary } from '$lib/types/content'

  interface Props {
    items: ContentSummary[]
    selectedId: string | null
    reorderable: boolean
    busyIds?: string[]
    onSelect: (id: string) => void
    onReorder: (ids: string[]) => Promise<void>
    onToggleSaved: (item: ContentSummary) => void
    onCopy: (item: ContentSummary) => void
    onDelete: (item: ContentSummary) => void
  }

  let { items, selectedId, reorderable, busyIds = [], onSelect, onReorder, onToggleSaved, onCopy, onDelete }: Props = $props()

  let dragging = $state<string | null>(null)

  async function keydown(e: KeyboardEvent) {
    const i = items.findIndex((x) => x.id === selectedId)
    let next: string | undefined
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      next = items[Math.min(items.length - 1, i + 1)]?.id
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      next = items[Math.max(0, i < 0 ? 0 : i - 1)]?.id
    } else if (e.key === 'Enter' && selectedId) {
      e.preventDefault()
      next = selectedId
    }
    if (next) {
      onSelect(next)
      // Keep the selection visible while keyboard-navigating.
      await tick()
      const el = document.querySelector(`[data-content-id="${CSS.escape(next)}"]`)
      // jsdom (unit tests) does not implement scrollIntoView.
      if (typeof el?.scrollIntoView === 'function') el.scrollIntoView({ block: 'nearest' })
    }
  }

  async function drop(id: string) {
    if (!dragging || dragging === id) return
    const ids = items.map((x) => x.id)
    const from = ids.indexOf(dragging)
    const to = ids.indexOf(id)
    ids.splice(to, 0, ids.splice(from, 1)[0])
    dragging = null
    await onReorder(ids)
  }
</script>

<div class="list" role="listbox" tabindex="0" onkeydown={keydown}>
  {#each items as item (item.id)}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      role="option"
      tabindex="-1"
      aria-selected={selectedId === item.id}
      data-content-id={item.id}
      draggable={reorderable && item.capabilities.reorder}
      ondragstart={() => (dragging = item.id)}
      ondragover={(e) => {
        if (reorderable) e.preventDefault()
      }}
      ondrop={() => drop(item.id)}
    >
      <ContentSummaryCard {item} selected={selectedId === item.id} busy={busyIds.includes(item.id)} draggable={reorderable} {onSelect} {onToggleSaved} {onCopy} {onDelete} />
    </div>
  {/each}
</div>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 0.38rem;
    outline: none;
  }

  .list:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
</style>
