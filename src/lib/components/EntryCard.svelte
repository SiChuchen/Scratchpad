<script lang="ts">
  import type { DockEntry } from '$lib/types/dock'
  import TextEntryBody from './entry/TextEntryBody.svelte'
  import ImageEntryBody from './entry/ImageEntryBody.svelte'
  import FileEntryBody from './entry/FileEntryBody.svelte'

  interface Props {
    entry: DockEntry
    onToggleCollapse: (entryId: string) => void
    onDeleteFromView: (entryId: string) => void
    onAddToNote: (entryId: string) => void
    onUpdateText: (id: string, content: string) => void
    onRename: (id: string, title: string | null) => void
    onCopy: (content: string) => void
    onCopyPath: (path: string) => void
  }

  let {
    entry,
    onToggleCollapse,
    onDeleteFromView,
    onAddToNote,
    onUpdateText,
    onRename,
    onCopy,
    onCopyPath,
  }: Props = $props()

  let editingTitle = $state(false)
  let titleDraft = $state('')
  let composing = $state(false)

  function autofocus(el: HTMLInputElement) {
    setTimeout(() => el.focus(), 0)
  }

  function startRename(e: MouseEvent) {
    e.stopPropagation()
    e.preventDefault()
    titleDraft = entry.title || ''
    editingTitle = true
  }

  function commitRename() {
    if (!editingTitle || composing) return
    editingTitle = false
    const trimmed = titleDraft.trim()
    const newTitle = trimmed || null
    if (newTitle !== entry.title) {
      onRename(entry.id, newTitle)
    }
  }

  function cancelRename() {
    editingTitle = false
  }

  function handleTitleKeydown(e: KeyboardEvent) {
    e.stopPropagation()
    if (e.isComposing) return
    if (e.key === 'Enter') commitRename()
    if (e.key === 'Escape') cancelRename()
  }

  function handleCompositionEnd() {
    composing = false
  }

  function handleDblClick() {
    if (editingTitle) return
    onToggleCollapse(entry.id)
  }

  function handleCollapsedCopy() {
    if (entry.kind === 'text') {
      onCopy(entry.content || '')
    } else if (entry.filePath) {
      onCopyPath(entry.filePath)
    }
  }

  let kindLabel = $derived.by(() => {
    switch (entry.kind) {
      case 'text': return '文本'
      case 'image': return '图片'
      case 'file': return '文件'
    }
  })

  let sourceLabel = $derived.by(() => {
    switch (entry.source) {
      case 'clipboard': return '剪贴板'
      case 'drop': return '拖入'
      default: return ''
    }
  })

  let timeLabel = $derived.by(() => {
    try {
      const d = new Date(entry.createdAt)
      const mm = String(d.getMonth() + 1).padStart(2, '0')
      const dd = String(d.getDate()).padStart(2, '0')
      const hh = String(d.getHours()).padStart(2, '0')
      const mi = String(d.getMinutes()).padStart(2, '0')
      return `${mm}/${dd} ${hh}:${mi}`
    } catch {
      return ''
    }
  })

  let previewText = $derived.by(() => {
    if (entry.collapsed) {
      if (entry.kind === 'text') {
        const lines = (entry.content || '').split(/\r?\n/)
        const firstLine = lines.find(l => l.trim()) || ''
        return firstLine.length > 10 ? firstLine.slice(0, 10) + '…' : firstLine
      }
      if (entry.fileName) return entry.fileName
      return ''
    }
    return ''
  })
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<article class="entry-card" class:collapsed={entry.collapsed} ondblclick={handleDblClick}>
  <header class="entry-header">
    <span class="kind-badge" class:text={entry.kind === 'text'} class:image={entry.kind === 'image'} class:file={entry.kind === 'file'}>
      {kindLabel}
    </span>
    {#if sourceLabel}
      <span class="source-badge">{sourceLabel}</span>
    {/if}
    {#if entry.collapsed}
      {#if editingTitle}
        <input
          class="title-edit-input"
          type="text"
          bind:value={titleDraft}
          use:autofocus
          onmousedown={(e) => e.stopPropagation()}
          onclick={(e) => e.stopPropagation()}
          onblur={commitRename}
          onkeydown={handleTitleKeydown}
          oncompositionstart={() => composing = true}
          oncompositionend={handleCompositionEnd}
        />
      {:else if entry.title || previewText}
        <span class="entry-preview" onclick={startRename}>{entry.title || previewText}</span>
        <button class="icon-btn copy-collapsed" onclick={() => handleCollapsedCopy()} title="复制">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
            <rect x="9" y="9" width="13" height="13" rx="2" />
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
          </svg>
        </button>
      {/if}
    {:else if entry.title}
      <span class="entry-title" onclick={startRename}>{entry.title}</span>
    {/if}
    <span class="entry-time">{timeLabel}</span>
    <div class="entry-header-actions">
      <button class="icon-btn" onclick={() => onToggleCollapse(entry.id)} title={entry.collapsed ? '展开' : '收起'}>
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          {#if entry.collapsed}
            <polyline points="6 9 12 15 18 9" />
          {:else}
            <polyline points="18 15 12 9 6 15" />
          {/if}
        </svg>
      </button>
      <button class="icon-btn note-btn" onclick={() => onAddToNote(entry.id)} disabled={entry.inNote} title={entry.inNote ? '已在 Note 中' : '收藏到 Note'}>
        <svg width="11" height="11" viewBox="0 0 24 24" fill={entry.inNote ? 'currentColor' : 'none'} stroke="currentColor" stroke-width="1.5">
          <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
        </svg>
      </button>
      <button class="icon-btn danger" onclick={() => onDeleteFromView(entry.id)} title="删除">
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <polyline points="3 6 5 6 21 6" />
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
        </svg>
      </button>
    </div>
  </header>

  {#if !entry.collapsed}
    <div class="entry-body">
      {#if entry.kind === 'text'}
        <TextEntryBody {entry} {onUpdateText} {onCopy} />
      {:else if entry.kind === 'image'}
        <ImageEntryBody {entry} {onCopy} {onCopyPath} />
      {:else}
        <FileEntryBody {entry} {onCopyPath} />
      {/if}
    </div>
  {/if}
</article>

<style>
  .entry-card {
    background: var(--surface-1);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg, 0.5rem);
    padding: 0.4rem 0.55rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    transition: border-color 0.12s;
  }

  .entry-card:hover {
    border-color: var(--border-emphasis);
  }

  .entry-card.collapsed {
    padding: 0.25rem 0.45rem;
  }

  .entry-card.collapsed .entry-header-actions .icon-btn {
    width: 1.4rem;
    height: 1.4rem;
  }

  .entry-header {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .kind-badge {
    padding: 0.1rem 0.4rem;
    border-radius: var(--radius-sm, 0.2rem);
    font-size: var(--font-xs, 0.55rem);
    font-weight: 600;
    letter-spacing: 0.03em;
    flex-shrink: 0;
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 20%, transparent);
    color: var(--color-primary);
  }

  .kind-badge.image {
    background: color-mix(in srgb, var(--color-info) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-info) 20%, transparent);
    color: var(--color-info);
  }

  .kind-badge.file {
    background: color-mix(in srgb, var(--color-file) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-file) 20%, transparent);
    color: var(--color-file);
  }

  .source-badge {
    padding: 0.0625rem 0.3rem;
    border-radius: 0.1875rem;
    font-size: var(--font-xs, 0.5rem);
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-accent) 20%, transparent);
    color: var(--color-accent);
  }

  .entry-preview {
    font-size: var(--font-sm, 0.65rem);
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
    cursor: pointer;
  }

  .entry-preview:hover {
    color: var(--text-primary);
  }

  .entry-time {
    font-size: var(--font-xs, 0.55rem);
    color: var(--text-faint);
    flex-shrink: 0;
  }

  .entry-header-actions {
    display: flex;
    gap: 0.15rem;
    margin-left: auto;
  }

  .icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.6rem;
    height: 1.6rem;
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--text-primary) 15%, transparent);
    border-radius: var(--radius-sm, 0.25rem);
    color: color-mix(in srgb, var(--text-primary) 55%, transparent);
    cursor: pointer;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
  }

  .icon-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 18%, transparent);
    border-color: color-mix(in srgb, var(--text-primary) 30%, transparent);
  }

  .icon-btn.note-btn {
    color: var(--color-accent);
    border-color: color-mix(in srgb, var(--color-accent) 20%, transparent);
  }

  .icon-btn.note-btn:disabled {
    color: color-mix(in srgb, var(--color-accent) 35%, transparent);
    cursor: default;
  }

  .icon-btn.note-btn:not(:disabled):hover {
    background: color-mix(in srgb, var(--color-accent) 15%, transparent);
    border-color: color-mix(in srgb, var(--color-accent) 35%, transparent);
  }

  .icon-btn.danger:hover {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-danger) 25%, transparent);
  }

  .copy-collapsed {
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-primary) 25%, transparent);
    color: var(--color-primary);
    width: 1.5rem;
    height: 1.5rem;
    border-radius: var(--radius-sm, 0.25rem);
    flex-shrink: 0;
  }

  .copy-collapsed:hover {
    background: color-mix(in srgb, var(--color-primary) 20%, transparent);
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
  }

  .title-edit-input {
    flex: 1;
    min-width: 0;
    background: var(--surface-2);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius-sm, 0.2rem);
    color: var(--text-primary);
    font-size: var(--font-xs, 0.55rem);
    font-family: inherit;
    padding: 0.1rem 0.3rem;
    outline: none;
  }

  .title-edit-input:focus {
    border-color: color-mix(in srgb, var(--color-primary) 50%, transparent);
  }

  .entry-title {
    font-size: var(--font-xs, 0.55rem);
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
    cursor: pointer;
  }

  .entry-title:hover {
    color: var(--text-primary);
  }
</style>
