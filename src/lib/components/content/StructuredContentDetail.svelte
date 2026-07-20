<script lang="ts">
  import { contentApi } from '$lib/api/content'
  import { vaultApi } from '$lib/api/vault'
  import { messages } from '$lib/i18n'
  import Icon from '$lib/components/Icon.svelte'
  import type { ContentDetail } from '$lib/types/content'

  type Structured = Extract<ContentDetail, { kind: 'credential' | 'bookmark' | 'note' }>

  interface Props {
    detail: Structured
    resetToken: string | number
    onClose: () => void
    onChanged: (id: string) => Promise<void>
    onNotify: (m: string, k?: 'success' | 'error') => void
    onDelete?: () => void
    onToggleSaved?: () => void
  }

  let { detail, resetToken, onClose, onChanged, onNotify, onDelete, onToggleSaved }: Props = $props()

  let revealed = $state<string[]>([])
  let editing = $state(false)
  let title = $state('')
  let notes = $state('')
  let body = $state('')
  let loadedId = $state('')

  $effect(() => {
    resetToken
    revealed = []
  })
  $effect(() => {
    if (loadedId !== detail.summary.id) {
      loadedId = detail.summary.id
      title = detail.summary.title
      notes = 'notes' in detail ? (detail.notes ?? '') : ''
      body = detail.kind === 'note' ? detail.body : ''
      editing = false
    }
  })

  function value(field: { key: string; value: string; isSensitive: boolean }) {
    return field.isSensitive && !revealed.includes(field.key) ? '••••••••' : field.value
  }

  async function copy(text: string, sensitive = false) {
    try {
      await vaultApi.copyText(text, sensitive)
      onNotify(messages.workspace.copied)
    } catch (e) {
      onNotify(String(e), 'error')
    }
  }

  async function save() {
    try {
      const fields =
        'fields' in detail
          ? detail.fields.map((f) => ({ key: f.key, value: f.value, isSensitive: f.isSensitive }))
          : []
      await contentApi.updateStructured(detail.summary.id, {
        kind: detail.kind,
        title,
        fields,
        notes: detail.kind === 'note' ? body : notes || null,
        manualTags: detail.tags.filter((t) => t.source === 'manual').map((t) => t.tag),
      })
      editing = false
      await onChanged(detail.summary.id)
      onNotify(messages.workspace.updated)
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
    {#if detail.kind === 'note'}
      <label>{messages.workspace.noteBody}<textarea bind:value={body}></textarea></label>
    {:else}
      <label>{messages.workspace.notes}<textarea bind:value={notes}></textarea></label>
    {/if}
    <div class="actions">
      <button type="button" class="btn ghost" onclick={() => (editing = false)}>{messages.workspace.cancel}</button>
      <button type="button" class="btn primary" onclick={save}>{messages.workspace.saveEdit}</button>
    </div>
  {:else}
    <section aria-label={messages.workspace.usefulInformation}>
      <h3>{messages.workspace.usefulInformation}</h3>
      {#if detail.kind === 'bookmark'}
        <div class="field" data-field-row>
          <span>{messages.workspace.linkLabel}</span>
          <a href={detail.url}>{detail.url}</a>
          <button data-copy-action type="button" class="btn small" onclick={() => copy(detail.url)}>{messages.workspace.copy}</button>
        </div>
      {/if}
      {#if detail.kind === 'note'}
        <div class="note-body">{detail.body}</div>
      {/if}
      {#if 'fields' in detail}
        {#each [...detail.fields].sort((a, b) => a.sortOrder - b.sortOrder) as field}
          <div class="field" data-field-row>
            <span>{field.key}</span>
            <strong>{value(field)}</strong>
            {#if field.isSensitive}
              <button
                type="button"
                class="btn small"
                onclick={() =>
                  (revealed = revealed.includes(field.key)
                    ? revealed.filter((k) => k !== field.key)
                    : [...revealed, field.key])}
              >{revealed.includes(field.key) ? messages.workspace.hide : messages.workspace.reveal}</button>
            {/if}
            <button data-copy-action class="btn small copy" type="button" onclick={() => copy(field.value, field.isSensitive)}>{messages.workspace.copy}</button>
          </div>
        {/each}
      {/if}
    </section>
    {#if 'notes' in detail && detail.notes}
      <section>
        <h3>{messages.workspace.notes}</h3>
        <p>{detail.notes}</p>
      </section>
    {/if}
    {#if detail.tags.length}
      <section>
        <h3>{messages.workspace.tags}</h3>
        <p>{detail.tags.map((t) => t.tag).join(' · ')}</p>
      </section>
    {/if}
    <div class="actions">
      <button type="button" class="btn primary" onclick={() => (editing = true)}>
        {detail.kind === 'note' ? messages.workspace.editNote : messages.workspace.edit}
      </button>
      {#if detail.kind === 'bookmark'}
        <button type="button" class="btn" onclick={() => window.open(detail.url, '_blank')}>{messages.workspace.openLink}</button>
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
    background: var(--surface-0);
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
    transition: color 0.12s, background 0.12s;
  }

  .back-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
  }

  main {
    display: flex;
    flex-direction: column;
    gap: 0.8rem;
    padding: 0.8rem;
  }

  h3 {
    margin: 0.15rem 0 0.4rem;
    color: var(--color-primary);
    font-size: max(var(--font-sm, 0.72rem), 0.72rem);
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .field {
    display: grid;
    grid-template-columns: minmax(4rem, 0.5fr) minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 0.4rem;
    padding: 0.38rem 0;
    border-bottom: 1px solid var(--border-subtle);
    font-size: max(var(--font-md, 0.85rem), 0.85rem);
    line-height: 1.5;
  }

  .field > span {
    color: var(--text-muted);
  }

  .field strong,
  .field a {
    overflow-wrap: anywhere;
  }

  .field .copy {
    margin-inline-start: auto;
  }

  .note-body,
  p {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: max(var(--font-md, 0.85rem), 0.85rem);
    line-height: 1.5;
    margin: 0;
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
    transition: background 0.12s, border-color 0.12s, color 0.12s;
  }

  .btn:hover:not(:disabled) {
    background: var(--surface-2);
    border-color: var(--border-emphasis);
  }

  .btn.small {
    min-height: 1.9rem;
    padding: 0.2rem 0.5rem;
    font-size: max(var(--font-xs, 0.68rem), 0.68rem);
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
    min-height: 2.25rem;
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
    min-height: 7rem;
    resize: vertical;
  }

  @media (max-width: 360px) {
    .field {
      grid-template-columns: minmax(3rem, 0.4fr) minmax(0, 1fr) auto;
    }
    .field button:not(.copy) {
      grid-column: 2;
    }
    .field .copy {
      grid-column: 3;
      grid-row: 1 / span 2;
    }
  }
</style>
