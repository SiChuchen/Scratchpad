<script lang="ts">
  import type { DockEntry } from '$lib/types/dock'
  import { looksLikeCode } from '$lib/utils/title'

  interface Props {
    entry: DockEntry
    onUpdateText: (id: string, content: string) => void
    onCopy: (content: string) => void
  }

  let { entry, onUpdateText, onCopy }: Props = $props()

  let editing = $state(false)
  let editValue = $state('')
  let isCode = $derived(looksLikeCode(entry.content || ''))
  let textareaEl: HTMLTextAreaElement | undefined = $state()

  function startEdit() {
    editValue = entry.content || ''
    editing = true
  }

  function commitEdit() {
    editing = false
    if (editValue !== (entry.content || '')) {
      onUpdateText(entry.id, editValue)
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      editing = false
    }
  }

  function autoResize(el: HTMLTextAreaElement) {
    el.style.height = 'auto'
    el.style.height = el.scrollHeight + 'px'
  }

  $effect(() => {
    if (editing && textareaEl) {
      autoResize(textareaEl)
    }
  })
</script>

{#if editing}
  <textarea
    bind:this={textareaEl}
    class="entry-edit"
    value={editValue}
    oninput={(e) => {
      editValue = (e.target as HTMLTextAreaElement).value
      autoResize(e.target as HTMLTextAreaElement)
    }}
    onkeydown={handleKeydown}
    onblur={commitEdit}
  ></textarea>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="entry-content" class:mono={isCode} onclick={startEdit}>
    {entry.content || '(空)'}
  </div>
{/if}

<div class="entry-actions">
  <button class="action-btn copy-action" onclick={() => onCopy(entry.content || '')}>
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
    <span>复制</span>
  </button>
</div>

<style>
  .entry-content {
    color: var(--text-primary);
    line-height: 1.45;
    padding: 0.2rem 0;
    cursor: text;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .entry-content.mono {
    font-family: 'Cascadia Code', 'Consolas', 'Courier New', monospace;
    font-size: 0.85em;
    line-height: 1.55;
    background: var(--surface-2);
    border-radius: var(--radius-md, 0.3rem);
    padding: 0.3rem 0.4rem;
  }

  .entry-edit {
    background: var(--surface-2);
    border: 1px solid color-mix(in srgb, var(--color-primary) 20%, transparent);
    border-radius: var(--radius-md, 0.375rem);
    color: var(--text-primary);
    line-height: 1.45;
    padding: 0.3rem 0.4rem;
    resize: none;
    min-height: 2rem;
    width: 100%;
    font-family: inherit;
    font-size: inherit;
    overflow: hidden;
    user-select: text;
  }

  .entry-edit:focus {
    outline: none;
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
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
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-md, 0.35rem);
    color: var(--text-faint);
    padding: 0.25rem 0.55rem;
    font-size: var(--font-xs, 0.6rem);
    cursor: pointer;
    transition: background 0.12s, color 0.12s, border-color 0.12s;
    font-family: inherit;
  }

  .action-btn:hover {
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
    border-color: color-mix(in srgb, var(--text-primary) 12%, transparent);
    color: var(--text-primary);
  }

  .copy-action:hover {
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-primary) 20%, transparent);
    color: var(--color-primary);
  }
</style>
