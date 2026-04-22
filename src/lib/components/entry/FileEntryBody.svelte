<script lang="ts">
  import type { DockEntry } from '$lib/types/dock'

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
  {#if entry.filePath}
    <button class="action-btn copy-action" onclick={() => onCopyPath(entry.filePath!)} title="复制路径">
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
    background: rgba(15, 23, 42, 0.4);
    border-radius: 0.375rem;
    color: rgba(148, 163, 184, 0.5);
  }

  .file-name {
    font-size: 0.65rem;
    color: rgba(225, 238, 247, 0.75);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
  }

  .file-size {
    font-size: 0.5rem;
    color: rgba(148, 163, 184, 0.45);
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
    background: color-mix(in srgb, var(--dock-text-color) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--dock-text-color) 15%, transparent);
    border-radius: 0.35rem;
    color: color-mix(in srgb, var(--dock-text-color) 60%, transparent);
    padding: 0.3rem 0.65rem;
    font-size: 0.65rem;
    cursor: pointer;
    transition: background 0.12s, color 0.12s;
    font-family: inherit;
  }

  .action-btn:hover {
    background: color-mix(in srgb, var(--dock-text-color) 18%, transparent);
    color: var(--dock-text-color);
  }

  .copy-action {
    background: rgba(125, 211, 252, 0.12);
    border-color: rgba(125, 211, 252, 0.25);
    color: rgba(125, 211, 252, 0.85);
    font-weight: 500;
  }

  .copy-action:hover {
    background: rgba(125, 211, 252, 0.22);
    border-color: rgba(125, 211, 252, 0.4);
    color: rgba(125, 211, 252, 1);
  }
</style>
