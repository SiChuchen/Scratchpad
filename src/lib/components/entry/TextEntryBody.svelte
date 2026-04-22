<script lang="ts">
  import type { DockEntry } from '$lib/types/dock'

  interface Props {
    entry: DockEntry
    onUpdateText: (id: string, content: string) => void
    onCopy: (content: string) => void
  }

  let { entry, onUpdateText, onCopy }: Props = $props()

  let editing = $state(false)
  let editValue = $state('')
  let mono = $state(false)
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
  <div class="entry-content" class:mono onclick={startEdit}>
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
  <button class="action-btn" class:active={mono} onclick={() => mono = !mono} title={mono ? '等宽字体' : '切换等宽字体'}>
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
      <polyline points="4 7 4 4 20 4 20 7" />
      <line x1="9" y1="20" x2="15" y2="20" />
      <line x1="12" y1="4" x2="12" y2="20" />
    </svg>
  </button>
</div>

<style>
  .entry-content {
    color: rgba(225, 238, 247, 0.85);
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
    background: rgba(15, 23, 42, 0.35);
    border-radius: 0.3rem;
    padding: 0.3rem 0.4rem;
  }

  .entry-edit {
    background: rgba(15, 23, 42, 0.5);
    border: 1px solid rgba(125, 211, 252, 0.2);
    border-radius: 0.375rem;
    color: #e5eef7;
    line-height: 1.45;
    padding: 0.3rem 0.4rem;
    resize: none;
    min-height: 2rem;
    width: 100%;
    font-family: inherit;
    font-size: inherit;
    overflow: hidden;
  }

  .entry-edit:focus {
    outline: none;
    border-color: rgba(125, 211, 252, 0.4);
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

  .action-btn.active {
    color: rgba(125, 211, 252, 0.9);
    background: rgba(125, 211, 252, 0.12);
    border-color: rgba(125, 211, 252, 0.25);
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
