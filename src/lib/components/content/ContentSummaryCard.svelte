<script lang="ts">
  import ContentKindIcon from './ContentKindIcon.svelte'
  import Icon from '$lib/components/Icon.svelte'
  import { messages } from '$lib/i18n'
  import { imageThumbnailUrl } from '$lib/api/thumbnails'
  import type { ContentSummary } from '$lib/types/content'

  interface Props {
    item: ContentSummary
    selected: boolean
    busy: boolean
    draggable?: boolean
    onSelect: (id: string) => void
    onToggleSaved: (item: ContentSummary) => void
    onCopy: (item: ContentSummary) => void
    onDelete: (item: ContentSummary) => void
    onDragHandle?: (id: string, event: PointerEvent) => void
    onCopyPath?: (item: ContentSummary) => void
  }

  let { item, selected, busy, draggable = false, onSelect, onToggleSaved, onCopy, onDelete, onDragHandle, onCopyPath }: Props = $props()

  // 图片条目在列表中恢复缩略图（旧版 EntryCard 行为）；详情接口按需懒加载并缓存。
  let thumbUrl = $state<string | null>(null)
  $effect(() => {
    if (item.kind !== 'image') {
      thumbUrl = null
      return
    }
    let cancelled = false
    imageThumbnailUrl(item.id).then((url) => {
      if (!cancelled) thumbUrl = url
    })
    return () => {
      cancelled = true
    }
  })

  const kindLabel = $derived(messages.workspace.kind[item.kind])
  const copyable = $derived(
    item.capabilities.copyText ||
      item.capabilities.copyImage ||
      item.capabilities.copyFile ||
      item.capabilities.copyPath,
  )
  const retentionLabel = $derived(
    item.retention === 'saved'
      ? messages.workspace.savedLabel
      : item.cleanupAt
        ? `${messages.workspace.cleanupUntil} ${new Date(item.cleanupAt).toLocaleDateString()}`
        : messages.workspace.temporary,
  )
  const selectLabel = $derived(
    messages.workspace.openItem.replace('{title}', item.title || messages.workspace.untitled),
  )

  function stop(action: () => void) {
    return (event: MouseEvent) => {
      event.stopPropagation()
      action()
    }
  }
</script>

<article class="card" class:selected aria-current={selected ? 'true' : undefined}>
  <button class="select" type="button" onclick={() => onSelect(item.id)} aria-label={selectLabel}>
    {#if item.kind === 'image'}
      {#if thumbUrl}
        <img class="thumb-banner" src={thumbUrl} alt="" draggable="false" />
      {:else}
        <div class="thumb-banner thumb-skeleton" aria-hidden="true"></div>
      {/if}
    {:else}
      <ContentKindIcon kind={item.kind} />
    {/if}
    <span class="main">
      <strong>{item.title || messages.workspace.untitled}</strong>
      {#if item.preview}<span class="preview">{item.preview}</span>{/if}
    </span>
  </button>
  <div class="meta"><span>{kindLabel}</span><span>{retentionLabel}</span></div>
  <div class="actions">
    {#if draggable && item.capabilities.reorder}
      <button type="button" class="icon-btn drag" aria-label={messages.workspace.dragToReorder} title={messages.workspace.dragToReorder} disabled={busy} onpointerdown={(e) => onDragHandle?.(item.id, e)}>
        <Icon name="grip" size={12} />
      </button>
    {/if}
    {#if copyable}
      <button type="button" class="icon-btn" aria-label={messages.workspace.copy} title={messages.workspace.copy} onclick={stop(() => onCopy(item))} disabled={busy}>
        <Icon name="copy" size={13} />
      </button>
    {/if}
    {#if item.capabilities.copyPath && onCopyPath}
      <button type="button" class="icon-btn" aria-label={messages.entry.copyPath} title={messages.entry.copyPath} onclick={stop(() => onCopyPath(item))} disabled={busy}>
        <Icon name="link" size={13} />
      </button>
    {/if}
    {#if item.capabilities.save}
      <button type="button" class="icon-btn star" aria-pressed="false" aria-label={messages.workspace.save} title={messages.workspace.save} onclick={stop(() => onToggleSaved(item))} disabled={busy}>
        <Icon name="star" size={13} />
      </button>
    {/if}
    {#if item.capabilities.unsave}
      <button type="button" class="icon-btn star active" aria-pressed="true" aria-label={messages.workspace.unsave} title={messages.workspace.unsave} onclick={stop(() => onToggleSaved(item))} disabled={busy}>
        <Icon name="star" size={13} filled />
      </button>
    {/if}
    {#if item.capabilities.delete}
      <button type="button" class="icon-btn danger" aria-label={messages.workspace.delete} title={messages.workspace.delete} onclick={stop(() => onDelete(item))} disabled={busy}>
        <Icon name="trash" size={13} />
      </button>
    {/if}
  </div>
</article>

<style>
  .card {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 0.2rem 0.5rem;
    padding: 0.55rem;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg, 0.5rem);
    background: var(--surface-1);
    cursor: pointer;
    overflow: hidden;
    transition:
      border-color var(--dur-fast, 120ms) var(--ease-out, ease-out),
      background var(--dur-fast, 120ms) var(--ease-out, ease-out),
      box-shadow var(--dur-fast, 120ms) var(--ease-out, ease-out),
      transform var(--dur-fast, 120ms) var(--ease-out, ease-out);
  }

  .card:hover {
    border-color: var(--border-emphasis);
    background: color-mix(in srgb, var(--text-primary) 3%, var(--surface-1));
  }

  /* Press feedback that does not shift layout bounds. */
  .card:active {
    transform: scale(0.995);
  }

  /* Selected is visually distinct from hover: primary tint + left indicator. */
  .card.selected {
    border-color: color-mix(in srgb, var(--color-primary) 45%, transparent);
    background: color-mix(in srgb, var(--color-primary) 8%, var(--surface-1));
    box-shadow: inset 2px 0 0 var(--color-primary);
  }

  .select {
    min-width: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0;
    border: 0;
    color: inherit;
    background: none;
    text-align: start;
    font: inherit;
    cursor: pointer;
  }

  .main {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.12rem;
  }

  strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: max(var(--font-md, 0.82rem), 0.82rem);
  }

  .preview {
    display: -webkit-box;
    overflow: hidden;
    line-clamp: 2;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    color: var(--text-muted);
    font-size: max(var(--font-sm, 0.72rem), 0.72rem);
    line-height: 1.4;
    overflow-wrap: anywhere;
  }

  .meta {
    grid-column: 1;
    display: flex;
    gap: 0.5rem;
    color: var(--text-faint);
    font-size: max(var(--font-xs, 0.65rem), 0.65rem);
  }

  .actions {
    grid-column: 2;
    grid-row: 1 / span 2;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 0.2rem;
  }

  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.7rem;
    height: 1.7rem;
    padding: 0;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md, 0.3rem);
    background: var(--surface-2);
    color: var(--text-muted);
    cursor: pointer;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
  }

  .icon-btn:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-emphasis);
    background: color-mix(in srgb, var(--text-primary) 10%, var(--surface-2));
  }

  .icon-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .icon-btn.drag {
    cursor: grab;
    touch-action: none;
  }

  /* 图片缩略图横幅：占满卡片宽度（不超出主界面框），裁切限高。
     未加载完成时渲染等高骨架，避免缩略图到达后的布局跳动。 */
  .thumb-banner {
    flex: 0 0 100%;
    width: 100%;
    height: 5.5rem;
    object-fit: cover;
    border-radius: var(--radius-md, 0.3rem);
    border: 1px solid var(--border-subtle);
    background: var(--surface-2);
    margin-bottom: 0.3rem;
  }

  .thumb-skeleton {
    background: linear-gradient(
      100deg,
      var(--surface-2) 40%,
      color-mix(in srgb, var(--text-primary) 7%, var(--surface-2)) 50%,
      var(--surface-2) 60%
    );
    background-size: 200% 100%;
    animation: thumb-shimmer 1.4s ease-in-out infinite;
  }

  @keyframes thumb-shimmer {
    from {
      background-position: 120% 0;
    }
    to {
      background-position: -80% 0;
    }
  }

  .icon-btn.star {
    color: var(--color-accent);
    border-color: color-mix(in srgb, var(--color-accent) 18%, transparent);
    background: color-mix(in srgb, var(--color-accent) 7%, var(--surface-2));
  }

  .icon-btn.star:hover:not(:disabled) {
    color: var(--color-accent);
    border-color: color-mix(in srgb, var(--color-accent) 32%, transparent);
    background: color-mix(in srgb, var(--color-accent) 14%, var(--surface-2));
  }

  .icon-btn.star.active {
    background: color-mix(in srgb, var(--color-accent) 16%, var(--surface-2));
    border-color: color-mix(in srgb, var(--color-accent) 30%, transparent);
  }

  .icon-btn.danger {
    color: color-mix(in srgb, var(--color-danger) 70%, var(--text-muted));
  }

  .icon-btn.danger:hover:not(:disabled) {
    color: var(--color-danger);
    border-color: color-mix(in srgb, var(--color-danger) 35%, transparent);
    background: color-mix(in srgb, var(--color-danger) 10%, var(--surface-2));
  }

  /* Keyboard-only focus ring: pointer clicks inside the card must not
     leave a lingering outline (focus-within would). */
  button:focus-visible,
  .card:has(:focus-visible) {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  @media (max-width: 360px) {
    .card {
      grid-template-columns: minmax(0, 1fr);
    }
    .actions {
      grid-column: 1;
      grid-row: auto;
      justify-content: flex-end;
    }
  }
</style>
