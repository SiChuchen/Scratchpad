<script lang="ts">
  import type { DockEntry } from '$lib/types/dock'
  import { dockApi } from '$lib/api/dock'

  interface Props {
    entry: DockEntry
    onCopy: (content: string) => void
    onCopyPath: (path: string) => void
  }

  let { entry, onCopy, onCopyPath }: Props = $props()

  let imageUrl = $derived.by(() => {
    if (entry.filePath) return dockApi.previewUrl(entry.filePath)
    return null
  })

  let sizeLabel = $derived.by(() => {
    const bytes = entry.sizeBytes
    if (bytes == null) return ''
    if (bytes < 1024) return `${bytes} B`
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  })

  let dimLabel = $derived.by(() => {
    if (entry.width && entry.height) return `${entry.width}x${entry.height}`
    return ''
  })

  let copyStatus = $state<'idle' | 'copying' | 'done'>('idle')

  async function copyImage() {
    if (!imageUrl || copyStatus === 'copying') return
    copyStatus = 'copying'
    try {
      const response = await fetch(imageUrl)
      const blob = await response.blob()
      await navigator.clipboard.write([
        new ClipboardItem({ [blob.type]: blob })
      ])
      copyStatus = 'done'
      setTimeout(() => { copyStatus = 'idle' }, 1500)
    } catch {
      copyStatus = 'idle'
    }
  }
</script>

{#if imageUrl}
  <div class="image-preview">
    <img src={imageUrl} alt={entry.fileName || '图片'} draggable="false" />
  </div>
{:else}
  <div class="image-placeholder">
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1" opacity="0.3">
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <circle cx="8.5" cy="8.5" r="1.5" />
      <polyline points="21 15 16 10 5 21" />
    </svg>
    {#if entry.fileName}
      <span class="image-filename">{entry.fileName}</span>
    {/if}
  </div>
{/if}

<div class="image-meta">
  {#if entry.fileName}
    <span class="meta-tag filename-tag">{entry.fileName}</span>
  {/if}
  {#if dimLabel}
    <span class="meta-tag">{dimLabel}</span>
  {/if}
  {#if sizeLabel}
    <span class="meta-tag">{sizeLabel}</span>
  {/if}
</div>

<div class="entry-actions">
  <button class="action-btn copy-action" onclick={copyImage} disabled={copyStatus === 'copying'} title="复制图片">
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
    <span>{copyStatus === 'done' ? '已复制' : '复制'}</span>
  </button>
  {#if entry.filePath}
    <button class="action-btn" onclick={() => onCopyPath(entry.filePath!)} title="复制路径">
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <rect x="9" y="9" width="13" height="13" rx="2" />
        <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
      </svg>
      <span>复制路径</span>
    </button>
  {/if}
</div>

<style>
  .image-preview {
    border-radius: var(--radius-md, 0.375rem);
    overflow: hidden;
    background: var(--surface-2);
  }

  .image-preview img {
    display: block;
    max-width: 100%;
    max-height: 10rem;
    object-fit: contain;
    border-radius: var(--radius-md, 0.375rem);
  }

  .image-placeholder {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem;
    background: var(--surface-2);
    border-radius: var(--radius-md, 0.375rem);
    color: var(--text-muted);
  }

  .image-filename {
    font-size: var(--font-sm, 0.65rem);
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .image-meta {
    display: flex;
    gap: 0.2rem;
    flex-wrap: wrap;
  }

  .meta-tag {
    background: var(--surface-1);
    padding: 0.0625rem 0.3rem;
    border-radius: 0.1875rem;
    font-size: var(--font-xs, 0.5rem);
    color: var(--text-muted);
  }

  .filename-tag {
    color: var(--text-primary);
  }

  .entry-actions {
    display: flex;
    gap: 0.2rem;
    align-items: center;
  }

  .action-btn {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--text-primary) 15%, transparent);
    border-radius: var(--radius-md, 0.35rem);
    color: color-mix(in srgb, var(--text-primary) 60%, transparent);
    padding: 0.3rem 0.65rem;
    font-size: var(--font-sm, 0.65rem);
    cursor: pointer;
    transition: background 0.12s, color 0.12s, border-color 0.12s;
    font-family: inherit;
  }

  .action-btn:hover {
    background: color-mix(in srgb, var(--text-primary) 18%, transparent);
    border-color: color-mix(in srgb, var(--text-primary) 25%, transparent);
    color: var(--text-primary);
  }

  .copy-action {
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-primary) 20%, transparent);
    color: var(--color-primary);
    font-weight: 500;
  }

  .copy-action:hover {
    background: color-mix(in srgb, var(--color-primary) 18%, transparent);
    border-color: color-mix(in srgb, var(--color-primary) 35%, transparent);
  }
</style>
