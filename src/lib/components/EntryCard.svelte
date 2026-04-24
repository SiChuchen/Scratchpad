<script lang="ts">
  import type { DockEntry } from '$lib/types/dock'
  import { generateTitle, looksLikeCode } from '$lib/utils/title'
  import TextEntryBody from './entry/TextEntryBody.svelte'
  import ImageEntryBody from './entry/ImageEntryBody.svelte'
  import FileEntryBody from './entry/FileEntryBody.svelte'

  interface Props {
    entry: DockEntry
    onToggleCollapse: (entryId: string) => void
    onDeleteFromView: (entryId: string) => void
    onToggleNote: (entryId: string) => void
    onUpdateText: (id: string, content: string) => void
    onRename: (id: string, title: string | null) => void
    onCopy: (content: string) => void
    onCopyPath: (path: string) => void
  }

  let {
    entry,
    onToggleCollapse,
    onDeleteFromView,
    onToggleNote,
    onUpdateText,
    onRename,
    onCopy,
    onCopyPath,
  }: Props = $props()

  let editingTitle = $state(false)
  let titleDraft = $state('')
  let composing = $state(false)
  let commitAfterComposition = $state(false)

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
    if (!editingTitle) return
    if (composing) {
      commitAfterComposition = true
      return
    }
    editingTitle = false
    commitAfterComposition = false
    const trimmed = titleDraft.trim()
    const newTitle = trimmed || null
    if (newTitle !== entry.title) {
      onRename(entry.id, newTitle)
    }
  }

  function cancelRename() {
    editingTitle = false
    composing = false
    commitAfterComposition = false
  }

  function handleTitleKeydown(e: KeyboardEvent) {
    e.stopPropagation()
    if (e.isComposing) return
    if (e.key === 'Enter') commitRename()
    if (e.key === 'Escape') cancelRename()
  }

  function handleCompositionEnd() {
    composing = false
    if (commitAfterComposition) commitRename()
  }

  function stop(e: MouseEvent) { e.stopPropagation() }

  let displayTitle = $derived(entry.title || generateTitle(entry) || '未命名条目')

  let showPreview = $derived.by(() => {
    if (!entry.collapsed) return false
    if (entry.kind !== 'text') return false
    const content = entry.content || ''
    if (!content) return false
    const lines = content.split(/\r?\n/)
    return lines.length > 3 || content.length > 80 || looksLikeCode(content)
  })

  let previewLine = $derived.by(() => {
    if (!showPreview) return ''
    const lines = (entry.content || '').split(/\r?\n/)
    if (entry.title) {
      const first = lines[0] || ''
      return first.length > 50 ? first.slice(0, 50) + '…' : first
    }
    const rest = lines.slice(1).find((l) => l.trim()) || ''
    return rest.length > 50 ? rest.slice(0, 50) + '…' : rest
  })

  let kindLabel = $derived.by(() => {
    switch (entry.kind) {
      case 'text': return '文本'
      case 'image': return '图片'
      case 'file': return '文件'
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

  function handleCopy(e: MouseEvent) {
    e.stopPropagation()
    if (entry.kind === 'text') onCopy(entry.content || '')
    else if (entry.filePath) onCopyPath(entry.filePath)
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<article class="entry-card" class:collapsed={entry.collapsed}>
  <header class="entry-header">
    <span class="kind-badge" class:text={entry.kind === 'text'} class:image={entry.kind === 'image'} class:file={entry.kind === 'file'}>
      {kindLabel}
    </span>
    {#if editingTitle}
      <input
        class="title-edit-input"
        type="text"
        bind:value={titleDraft}
        use:autofocus
        data-card-interactive
        onmousedown={(e) => e.stopPropagation()}
        onclick={(e) => e.stopPropagation()}
        ondblclick={stop}
        onblur={commitRename}
        onkeydown={handleTitleKeydown}
        oncompositionstart={() => composing = true}
        oncompositionend={handleCompositionEnd}
      />
    {:else}
      <button
        class="title-trigger"
        class:untitled={!entry.title}
        data-card-interactive
        onclick={startRename}
        ondblclick={stop}
      >{displayTitle}</button>
    {/if}
    {#if showPreview}
      <span class="preview-line">{previewLine}</span>
    {/if}
    <span class="entry-time">{timeLabel}</span>
    <div class="entry-header-actions" data-card-interactive ondblclick={stop}>
      <button class="icon-btn" onclick={(e) => { e.stopPropagation(); handleCopy(e) }} title="复制">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <rect x="9" y="9" width="13" height="13" rx="2" />
          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
        </svg>
      </button>
      <button class="icon-btn" onclick={(e) => { e.stopPropagation(); onToggleCollapse(entry.id) }} title={entry.collapsed ? '展开' : '收起'}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          {#if entry.collapsed}
            <polyline points="6 9 12 15 18 9" />
          {:else}
            <polyline points="18 15 12 9 6 15" />
          {/if}
        </svg>
      </button>
      <button class="icon-btn note-btn" class:active={entry.inNote} onclick={(e) => { e.stopPropagation(); onToggleNote(entry.id) }} title={entry.inNote ? '取消收藏' : '收藏'}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill={entry.inNote ? 'currentColor' : 'none'} stroke="currentColor" stroke-width="1.5">
          <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
        </svg>
      </button>
      <button class="icon-btn danger" onclick={(e) => { e.stopPropagation(); onDeleteFromView(entry.id) }} title="删除">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <polyline points="3 6 5 6 21 6" />
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
        </svg>
      </button>
    </div>
  </header>

  {#if !entry.collapsed}
    <div class="entry-body" data-card-interactive ondblclick={stop}>
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
    gap: 0.2rem;
    transition: border-color 0.12s;
    user-select: none;
    -webkit-user-drag: none;
  }

  .entry-card:hover {
    border-color: var(--border-emphasis);
  }

  .entry-header {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    min-height: 1.5rem;
  }

  .kind-badge {
    padding: 0.08rem 0.35rem;
    border-radius: var(--radius-sm, 0.2rem);
    font-size: 0.55rem;
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

  .title-trigger {
    background: none;
    border: 0;
    padding: 0;
    font-family: inherit;
    font-size: 0.72rem;
    color: var(--text-primary);
    line-height: 1.3;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 40%;
    flex: 0 1 auto;
    min-width: 0;
    cursor: pointer;
  }

  .title-trigger:hover {
    color: var(--color-primary);
  }

  .title-trigger.untitled {
    color: var(--text-muted);
  }

  .preview-line {
    flex: 1;
    min-width: 0;
    font-size: var(--font-xs, 0.5rem);
    color: var(--text-faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 1.2;
  }

  .entry-time {
    font-size: var(--font-xs, 0.5rem);
    color: var(--text-faint);
    flex-shrink: 0;
  }

  .entry-header-actions {
    display: flex;
    gap: 0.15rem;
    margin-left: auto;
    flex-shrink: 0;
  }

  .icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.5rem;
    height: 1.5rem;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm, 0.2rem);
    color: var(--text-muted);
    cursor: pointer;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
  }

  .icon-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
    border-color: color-mix(in srgb, var(--text-primary) 15%, transparent);
  }

  .icon-btn.note-btn.active {
    color: var(--color-accent);
  }

  .icon-btn.note-btn:hover {
    color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-accent) 20%, transparent);
  }

  .icon-btn.danger {
    color: var(--text-faint);
  }

  .icon-btn.danger:hover {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-danger) 20%, transparent);
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

  .entry-body {
    padding-top: 0.15rem;
  }
</style>
