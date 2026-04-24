<script lang="ts">
  import type { DockEntry } from '$lib/types/dock'
  import { dockApi } from '$lib/api/dock'

  interface Props {
    entry: DockEntry
    onCopyPath: (path: string) => void
  }

  let { entry, onCopyPath }: Props = $props()

  let sizeLabel = $derived.by(() => {
    const bytes = entry.sizeBytes
    if (bytes == null) return ''
    if (bytes < 1024) return `${bytes} B`
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  })

  let copyStatus = $state<'idle' | 'copying' | 'done'>('idle')

  async function copyFile() {
    if (!entry.filePath || copyStatus === 'copying') return
    copyStatus = 'copying'
    try {
      const url = dockApi.previewUrl(entry.filePath)
      const response = await fetch(url)
      const blob = await response.blob()
      const type = blob.type || 'application/octet-stream'
      // Try writing as ClipboardItem; fall back to text if unsupported type
      try {
        await navigator.clipboard.write([
          new ClipboardItem({ [type]: blob })
        ])
      } catch {
        const text = await blob.text()
        await navigator.clipboard.writeText(text)
      }
      copyStatus = 'done'
      setTimeout(() => { copyStatus = 'idle' }, 1500)
    } catch {
      copyStatus = 'idle'
    }
  }
</script>

<div class="file-info">
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1" opacity="0.4">
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <polyline points="14 2 14 8 20 8" />
  </svg>
  <span class="file-name">{entry.fileName || '未知文件'}</span>
  {#if sizeLabel}
    <span class="file-size">{sizeLabel}</span>
  {/if}
</div>

<div class="entry-actions">
  <button class="action-btn copy-action" onclick={copyFile} disabled={copyStatus === 'copying'} title="复制文件">
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
  .file-info {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.3rem;
    background: var(--surface-2);
    border-radius: var(--radius-md, 0.375rem);
    color: var(--text-muted);
  }

  .file-name {
    font-size: var(--font-sm, 0.65rem);
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
  }

  .file-size {
    font-size: var(--font-xs, 0.5rem);
    color: var(--text-faint);
    flex-shrink: 0;
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
