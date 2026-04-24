<script lang="ts">
  import type { DockEntry } from '$lib/types/dock'
  import EntryCard from '$lib/components/EntryCard.svelte'
  import { dockApi } from '$lib/api/dock'

  interface Props {
    entries: DockEntry[]
    onToggleCollapse: (entryId: string) => void
    onDeleteFromView: (entryId: string) => void
    onAddToNote: (entryId: string) => void
    onCreateText: (content: string) => void
    onImportEntry: (entry: DockEntry) => void
    onUpdateText: (id: string, content: string) => void
    onRename: (id: string, title: string | null) => void
    onCopy: (content: string) => void
    onCopyPath: (path: string) => void
  }

  let {
    entries,
    onToggleCollapse,
    onDeleteFromView,
    onAddToNote,
    onCreateText,
    onImportEntry,
    onUpdateText,
    onRename,
    onCopy,
    onCopyPath,
  }: Props = $props()

  let showNewForm = $state(false)
  let newText = $state('')
  let searchQuery = $state('')
  let dragIdx = $state<number | null>(null)
  let dragOverIdx = $state<number | null>(null)
  let bodyEl: HTMLDivElement | undefined = $state()

  function scrollToTop() {
    if (bodyEl) bodyEl.scrollTop = 0
  }

  let filtered = $derived(
    searchQuery.trim()
      ? entries.filter((e) => {
          const q = searchQuery.toLowerCase()
          return (
            (e.title && e.title.toLowerCase().includes(q)) ||
            (e.content && e.content.toLowerCase().includes(q)) ||
            (e.fileName && e.fileName.toLowerCase().includes(q))
          )
        })
      : entries,
  )

  function submitText() {
    const text = newText.trim()
    if (!text) return
    onCreateText(text)
    newText = ''
    showNewForm = false
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      submitText()
    }
    if (e.key === 'Escape') {
      showNewForm = false
      newText = ''
    }
  }

  // --- Reorder drag handlers ---
  function onCardDragStart(idx: number, event: DragEvent) {
    dragIdx = idx
    event.dataTransfer!.effectAllowed = 'move'
    event.dataTransfer!.setData('application/x-dock-reorder', String(idx))
    // Hide default drag ghost
    const ghost = document.createElement('canvas')
    ghost.width = 1
    ghost.height = 1
    event.dataTransfer!.setDragImage(ghost, 0, 0)
  }

  function onCardDragOver(idx: number, event: DragEvent) {
    if (event.dataTransfer?.types.includes('application/x-dock-reorder')) {
      event.preventDefault()
      dragOverIdx = idx
    }
  }

  function onCardDragLeave() {
    dragOverIdx = null
  }

  async function onCardDrop(idx: number, event: DragEvent) {
    if (!event.dataTransfer?.types.includes('application/x-dock-reorder')) return
    event.preventDefault()
    event.stopPropagation()
    if (dragIdx === null || dragIdx === idx || searchQuery.trim()) {
      dragIdx = null
      dragOverIdx = null
      return
    }
    const list = [...filtered]
    const [moved] = list.splice(dragIdx, 1)
    list.splice(idx, 0, moved)
    const orderedIds = list.map((e) => e.id)
    dragIdx = null
    dragOverIdx = null

    try {
      await dockApi.reorderEntries('home', orderedIds)
    } catch {
      // silently fail, order will be restored on next load
    }
  }

  function onCardDragEnd() {
    dragIdx = null
    dragOverIdx = null
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="home-view">
  <div class="home-header">
    <button class="add-btn" onclick={() => { showNewForm = !showNewForm; if (showNewForm) newText = '' }}>
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <line x1="12" y1="5" x2="12" y2="19" />
        <line x1="5" y1="12" x2="19" y2="12" />
      </svg>
    </button>
    {#if entries.length > 0}
      <div class="search-box">
        <svg class="search-icon" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="11" cy="11" r="8" />
          <line x1="21" y1="21" x2="16.65" y2="16.65" />
        </svg>
        <input
          type="text"
          class="search-input"
          placeholder="搜索..."
          bind:value={searchQuery}
        />
        {#if searchQuery}
          <button class="search-clear" onclick={() => searchQuery = ''}>x</button>
        {/if}
      </div>
    {/if}
  </div>

  {#if showNewForm}
    <div class="new-form">
      <textarea
        class="new-textarea"
        placeholder="输入文本内容..."
        bind:value={newText}
        onkeydown={handleKeydown}
        rows="3"
      ></textarea>
      <div class="new-form-actions">
        <button class="form-btn form-cancel" onclick={() => { showNewForm = false; newText = '' }}>取消</button>
        <button class="form-btn form-submit" onclick={submitText} disabled={!newText.trim()}>添加</button>
      </div>
    </div>
  {/if}

  <div class="home-body" bind:this={bodyEl}>
    {#if entries.length === 0}
      <div class="dock-empty">
        <p>暂无内容</p>
        <p class="hint">点击 + 添加文本片段</p>
        <p class="hint">Ctrl + 拖动移动窗口 | 粘贴或拖入文件</p>
      </div>
    {:else if filtered.length === 0}
      <div class="dock-empty">
        <p>未找到匹配内容</p>
      </div>
    {:else}
      <div class="entry-list">
        {#each filtered as entry, i (entry.id)}
          <div
            class="entry-wrapper"
            class:drag-over-top={dragOverIdx === i && dragIdx !== null && dragIdx > i}
            class:drag-over-bottom={dragOverIdx === i && dragIdx !== null && dragIdx < i}
            class:dragging={dragIdx === i}
            draggable="true"
            ondragstart={(e) => onCardDragStart(i, e)}
            ondragover={(e) => onCardDragOver(i, e)}
            ondragleave={onCardDragLeave}
            ondrop={(e) => onCardDrop(i, e)}
            ondragend={onCardDragEnd}
          >
            <EntryCard
              {entry}
              {onToggleCollapse}
              {onDeleteFromView}
              {onAddToNote}
              {onUpdateText}
              {onRename}
              {onCopy}
              {onCopyPath}
            />
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .home-view {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.5rem 0.65rem;
    overflow: hidden;
    min-height: 0;
  }

  .home-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex-shrink: 0;
    height: 1.75rem;
  }

  .add-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.75rem;
    height: 1.75rem;
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--text-primary) 20%, transparent);
    color: var(--text-primary);
    cursor: pointer;
    border-radius: var(--radius-md, 0.35rem);
    transition: color 0.12s, background 0.12s;
  }

  .add-btn:hover {
    background: color-mix(in srgb, var(--text-primary) 20%, transparent);
  }

  .search-box {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.25rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 0.3rem);
    padding: 0 0.35rem;
    height: 1.75rem;
  }

  .search-icon {
    flex-shrink: 0;
    opacity: 0.35;
  }

  .search-input {
    flex: 1;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: var(--font-sm, 0.6rem);
    font-family: inherit;
    outline: none;
    padding: 0;
    min-width: 0;
  }

  .search-input::placeholder {
    color: var(--text-faint);
  }

  .search-clear {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: var(--font-sm, 0.6rem);
    cursor: pointer;
    padding: 0;
    font-family: inherit;
  }

  .search-clear:hover {
    color: var(--text-primary);
  }

  .new-form {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.2rem 0;
    flex-shrink: 0;
  }

  .new-textarea {
    width: 100%;
    background: var(--surface-2);
    border: 1px solid color-mix(in srgb, var(--color-primary) 20%, transparent);
    border-radius: var(--radius-lg, 0.5rem);
    color: var(--text-primary);
    font-size: var(--font-sm, 0.7rem);
    font-family: inherit;
    line-height: 1.45;
    padding: 0.4rem 0.5rem;
    resize: vertical;
    min-height: 2.5rem;
    max-height: 6rem;
    outline: none;
  }

  .new-textarea::placeholder {
    color: var(--text-faint);
  }

  .new-textarea:focus {
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
  }

  .new-form-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.25rem;
  }

  .form-btn {
    padding: 0.2rem 0.6rem;
    border-radius: 0.375rem;
    font-size: 0.625rem;
    cursor: pointer;
    transition: background 0.12s, color 0.12s;
    font-family: inherit;
  }

  .form-cancel {
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    color: var(--text-muted);
  }

  .form-cancel:hover {
    background: var(--border-default);
    color: var(--text-primary);
  }

  .form-submit {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    color: var(--color-primary);
    font-weight: 500;
  }

  .form-submit:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-primary) 25%, transparent);
  }

  .form-submit:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .home-body {
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
    color: var(--text-faint);
    font-size: var(--font-sm, 0.75rem);
    text-align: center;
  }

  .dock-empty .hint {
    font-size: var(--font-sm, 0.65rem);
    color: var(--text-faint);
    margin-top: 0.2rem;
  }

  .entry-list {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .entry-wrapper {
    transition: opacity 0.15s;
  }

  .entry-wrapper.dragging {
    opacity: 0.4;
  }

  .entry-wrapper.drag-over-top {
    border-top: 2px solid color-mix(in srgb, var(--color-primary) 60%, transparent);
    padding-top: 0;
  }

  .entry-wrapper.drag-over-bottom {
    border-bottom: 2px solid color-mix(in srgb, var(--color-primary) 60%, transparent);
    padding-bottom: 0;
  }
</style>
