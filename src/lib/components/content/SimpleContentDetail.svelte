<script lang="ts">
  import { contentApi } from '$lib/api/content'
  import { dockApi } from '$lib/api/dock'
  import { messages } from '$lib/i18n'
  import Icon from '$lib/components/Icon.svelte'
  import type { ContentDetail } from '$lib/types/content'

  interface Props {
    detail: Extract<ContentDetail, { kind: 'text' | 'image' | 'file' }>
    onClose: () => void
    onChanged: (id: string) => Promise<void>
    onNotify: (m: string, k?: 'success' | 'error') => void
    onDelete?: () => void
    onToggleSaved?: () => void
  }

  let { detail, onClose, onChanged, onNotify, onDelete, onToggleSaved }: Props = $props()

  let editing = $state(false)
  let title = $state('')
  let body = $state('')
  let loadedId = $state('')

  $effect(() => {
    if (loadedId !== detail.summary.id) {
      loadedId = detail.summary.id
      title = detail.summary.title
      body = detail.kind === 'text' ? detail.body : ''
      editing = false
    }
  })

  const copyLabel = $derived(
    detail.kind === 'image'
      ? messages.entry.copyImage
      : detail.kind === 'file'
        ? messages.entry.copyFile
        : messages.workspace.copy,
  )

  const imageUrl = $derived(
    detail.kind === 'image' && detail.available ? dockApi.previewUrl(detail.assetPath) : null,
  )

  async function save() {
    try {
      if (detail.kind === 'text') await contentApi.updateText(detail.summary.id, title || null, body)
      else await contentApi.rename(detail.summary.id, title || null)
      editing = false
      await onChanged(detail.summary.id)
      onNotify(messages.workspace.updated)
    } catch (e) {
      onNotify(String(e), 'error')
    }
  }

  async function copy() {
    try {
      if (detail.kind === 'text') await navigator.clipboard.writeText(detail.body)
      else if (detail.kind === 'image' && detail.available) await dockApi.copyImage(detail.assetPath)
      else if (detail.kind === 'file' && detail.available) await dockApi.copyFile(detail.assetPath)
      onNotify(messages.workspace.copied)
    } catch (e) {
      onNotify(String(e), 'error')
    }
  }

  async function copyPath() {
    if (detail.kind !== 'image' && detail.kind !== 'file') return
    try {
      await navigator.clipboard.writeText(detail.assetPath)
      onNotify(messages.toast.copiedPath)
    } catch (e) {
      onNotify(String(e), 'error')
    }
  }

  async function revealInFolder() {
    if (detail.kind !== 'image' && detail.kind !== 'file') return
    try {
      await dockApi.revealInFolder(detail.assetPath)
    } catch (e) {
      onNotify(String(e), 'error')
    }
  }
</script>

<header>
  <button type="button" class="back-btn" onclick={onClose}>
    <Icon name="back" size={13} />
    <span>{messages.workspace.back}</span>
  </button>
  <strong>{detail.summary.title}</strong>
</header>
<main>
  {#if editing}
    <label>{messages.workspace.title}<input bind:value={title} /></label>
    {#if detail.kind === 'text'}
      <label>{messages.workspace.body}<textarea bind:value={body}></textarea></label>
    {/if}
    <div class="actions">
      <button type="button" class="btn ghost" onclick={() => (editing = false)}>{messages.workspace.cancel}</button>
      <button type="button" class="btn primary" onclick={save}>{messages.workspace.saveEdit}</button>
    </div>
  {:else}
    {#if detail.kind === 'text'}
      <pre>{detail.body}</pre>
    {:else if !detail.available}
      <p role="status">{detail.kind === 'image' ? messages.workspace.unavailableImage : messages.workspace.unavailableFile}</p>
    {:else}
      {#if detail.kind === 'image' && imageUrl}
        <img class="asset-preview" src={imageUrl} alt={detail.fileName} draggable="false" />
      {/if}
      <p class="path">{detail.fileName}</p>
    {/if}
    <div class="actions">
      <button type="button" class="btn primary" onclick={() => (editing = true)}>
        {detail.kind === 'text' ? messages.workspace.editText : messages.workspace.rename}
      </button>
      <button type="button" class="btn" onclick={copy} disabled={detail.kind !== 'text' && !detail.available}>{copyLabel}</button>
      {#if detail.kind !== 'text' && detail.summary.capabilities.copyPath}
        <button type="button" class="btn" onclick={copyPath} disabled={!detail.available}>{messages.entry.copyPath}</button>
        <button type="button" class="btn" onclick={revealInFolder} disabled={!detail.available}>{messages.workspace.revealInFolder}</button>
      {/if}
      {#if onToggleSaved}
        <button type="button" class="btn" onclick={onToggleSaved}>
          {detail.summary.retention === 'saved' ? messages.workspace.unsave : messages.workspace.save}
        </button>
      {/if}
      {#if onDelete}
        <button type="button" class="btn danger" onclick={onDelete}>{messages.workspace.delete}</button>
      {/if}
    </div>
  {/if}
</main>

<style>
  header {
    position: sticky;
    top: 0;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.55rem 0.6rem;
    border-bottom: 1px solid var(--border-subtle);
    background: color-mix(in srgb, var(--surface-0) 88%, transparent);
    backdrop-filter: blur(8px);
    z-index: 1;
  }

  header strong {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: max(var(--font-md, 0.85rem), 0.85rem);
  }

  .back-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    min-height: 2rem;
    padding: 0.25rem 0.55rem;
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: max(var(--font-sm, 0.75rem), 0.75rem);
    cursor: pointer;
    flex: 0 0 auto;
    transition:
      color var(--dur-fast, 120ms) var(--ease-out, ease-out),
      background var(--dur-fast, 120ms) var(--ease-out, ease-out);
  }

  .back-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
  }

  main {
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
    padding: 0.8rem;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    color: var(--text-muted);
    font-size: max(var(--font-sm, 0.75rem), 0.75rem);
  }

  input,
  textarea {
    box-sizing: border-box;
    width: 100%;
    min-height: 2.2rem;
    padding: 0.5rem;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--surface-1);
    color: var(--text-primary);
    font: inherit;
  }

  input:focus,
  textarea:focus {
    outline: none;
    border-color: color-mix(in srgb, var(--color-primary) 45%, transparent);
  }

  textarea {
    min-height: 8rem;
    resize: vertical;
  }

  pre,
  .path {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: inherit;
    font-size: max(var(--font-md, 0.85rem), 0.85rem);
    line-height: 1.55;
    margin: 0;
  }

  .asset-preview {
    display: block;
    max-width: 100%;
    max-height: 16rem;
    object-fit: contain;
    border-radius: var(--radius-md);
    border: 1px solid var(--border-subtle);
    background: var(--surface-2);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 0.35rem;
  }

  .btn {
    min-height: 2.25rem;
    padding: 0.35rem 0.7rem;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    background: var(--surface-1);
    color: var(--text-primary);
    font: inherit;
    font-size: max(var(--font-sm, 0.75rem), 0.75rem);
    cursor: pointer;
    transition:
      background var(--dur-fast, 120ms) var(--ease-out, ease-out),
      border-color var(--dur-fast, 120ms) var(--ease-out, ease-out),
      color var(--dur-fast, 120ms) var(--ease-out, ease-out),
      transform var(--dur-fast, 120ms) var(--ease-out, ease-out);
  }

  .btn:hover:not(:disabled) {
    background: var(--surface-2);
    border-color: var(--border-emphasis);
  }

  .btn:active:not(:disabled) {
    transform: scale(0.97);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .btn.primary {
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
    background: color-mix(in srgb, var(--color-primary) 14%, var(--surface-1));
    color: var(--color-primary);
    font-weight: 500;
  }

  .btn.primary:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-primary) 22%, var(--surface-1));
    border-color: color-mix(in srgb, var(--color-primary) 55%, transparent);
  }

  .btn.ghost {
    border-color: transparent;
    background: none;
    color: var(--text-muted);
  }

  .btn.ghost:hover:not(:disabled) {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
  }

  .btn.danger {
    color: var(--color-danger);
  }

  .btn.danger:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--color-danger) 35%, transparent);
    background: color-mix(in srgb, var(--color-danger) 10%, var(--surface-1));
  }
</style>
