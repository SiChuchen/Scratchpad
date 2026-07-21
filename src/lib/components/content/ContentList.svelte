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
    onCopyPath?: (item: ContentSummary) => void
  }

  let { items, selectedId, reorderable, busyIds = [], onSelect, onReorder, onToggleSaved, onCopy, onDelete, onCopyPath }: Props = $props()

  // 拖动排序使用指针事件而非 HTML5 DnD：主窗口 dragDropEnabled=true 时
  // Tauri 会接管拖拽（用于接收系统文件拖入），HTML5 dragstart 不会触发。
  let dragId = $state<string | null>(null)
  let dropTargetId = $state<string | null>(null)

  function cleanupDrag() {
    window.removeEventListener('pointermove', trackDrag)
    document.documentElement.style.userSelect = ''
  }

  function startDrag(id: string, e: PointerEvent) {
    if (!reorderable || e.button !== 0) return
    e.preventDefault()
    dragId = id
    dropTargetId = null
    document.documentElement.style.userSelect = 'none'
    window.addEventListener('pointermove', trackDrag)
    window.addEventListener('pointerup', endDrag, { once: true })
    window.addEventListener('pointercancel', cancelDrag, { once: true })
  }

  function trackDrag(e: PointerEvent) {
    if (!dragId) return
    const hit =
      typeof document.elementFromPoint === 'function'
        ? document.elementFromPoint(e.clientX, e.clientY)
        : null
    const overId = hit?.closest('[data-content-id]')?.getAttribute('data-content-id') ?? null
    dropTargetId = overId && overId !== dragId ? overId : null
  }

  async function endDrag() {
    const from = dragId
    const to = dropTargetId
    cleanupDrag()
    dragId = null
    dropTargetId = null
    if (!from || !to) return
    const ids = items.map((x) => x.id)
    const fromIndex = ids.indexOf(from)
    const toIndex = ids.indexOf(to)
    if (fromIndex < 0 || toIndex < 0) return
    ids.splice(toIndex, 0, ids.splice(fromIndex, 1)[0])
    await onReorder(ids)
  }

  function cancelDrag() {
    cleanupDrag()
    dragId = null
    dropTargetId = null
  }

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

</script>

<div class="list" role="listbox" tabindex="0" onkeydown={keydown}>
  {#each items as item (item.id)}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      role="option"
      tabindex="-1"
      aria-selected={selectedId === item.id}
      data-content-id={item.id}
      class:dragging={dragId === item.id}
      class:drop-target={dropTargetId === item.id}
    >
      <ContentSummaryCard {item} selected={selectedId === item.id} busy={busyIds.includes(item.id)} draggable={reorderable} {onSelect} {onToggleSaved} {onCopy} {onDelete} onDragHandle={startDrag} {onCopyPath} />
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

  .list > div {
    border-radius: var(--radius-lg, 0.5rem);
    transition: opacity 0.12s, box-shadow 0.12s;
  }

  .list > div.dragging {
    opacity: 0.45;
  }

  /* 落点指示：目标卡片上方一条主色线 */
  .list > div.drop-target {
    box-shadow: 0 -2px 0 0 var(--color-primary);
  }
</style>
