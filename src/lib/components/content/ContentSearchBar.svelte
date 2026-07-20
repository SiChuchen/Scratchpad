<script lang="ts">
  import type { ContentKind } from '$lib/types/content'
  import { messages } from '$lib/i18n'
  import Icon from '$lib/components/Icon.svelte'

  interface Props {
    query: string
    selectedKind: ContentKind | null
    searching?: boolean
    onSearch: (q: string) => void
    onClear: () => void
    onSetKind: (kind: ContentKind | null) => void
  }

  let { query, selectedKind, searching = false, onSearch, onClear, onSetKind }: Props = $props()

  const kinds: [ContentKind | null, string][] = $derived([
    [null, messages.workspace.allKinds],
    ['text', messages.workspace.kind.text],
    ['image', messages.workspace.kind.image],
    ['file', messages.workspace.kind.file],
    ['credential', messages.workspace.kind.credential],
    ['bookmark', messages.workspace.kind.bookmark],
    ['note', messages.workspace.kind.note],
  ])

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && query) {
      e.preventDefault()
      onClear()
    }
  }
</script>

<div class="search-row">
  <div class="input-wrap">
    <span class="search-icon" aria-hidden="true"><Icon name="search" size={13} /></span>
    <input
      type="search"
      value={query}
      aria-label={messages.workspace.searchPlaceholder}
      placeholder={messages.workspace.searchPlaceholder}
      oninput={(e) => onSearch(e.currentTarget.value)}
      onkeydown={keydown}
    />
    {#if query}
      <button type="button" class="clear-btn" aria-label={messages.workspace.clearSearch} title={messages.workspace.clearSearch} onclick={onClear}>
        <Icon name="x" size={11} />
      </button>
    {/if}
    {#if searching}
      <span class="busy" role="status">{messages.workspace.searching}</span>
    {/if}
  </div>
  <div class="chips" aria-label={messages.workspace.kindFilter}>
    {#each kinds as [kind, label]}
      <button
        type="button"
        class:active={selectedKind === kind}
        aria-pressed={selectedKind === kind}
        onclick={() => onSetKind(kind)}
      >{label}</button>
    {/each}
  </div>
</div>

<style>
  .search-row {
    position: sticky;
    top: 0;
    z-index: 5;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    padding: 0.45rem 0.55rem;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--surface-0);
  }

  .input-wrap {
    height: 2.35rem;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0 0.55rem;
    border: 1px solid var(--border-default);
    border-radius: 999px;
    background: var(--surface-1);
    transition: border-color 0.12s;
  }

  .input-wrap:focus-within {
    border-color: color-mix(in srgb, var(--color-primary) 45%, transparent);
  }

  .search-icon {
    display: inline-flex;
    color: var(--text-faint);
    flex: 0 0 auto;
  }

  input {
    min-width: 0;
    flex: 1;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--text-primary);
    font: inherit;
    font-size: max(var(--font-md, 0.82rem), 0.82rem);
  }

  input::placeholder {
    color: var(--text-faint);
  }

  /* Hide the native clear control — a custom one is rendered instead. */
  input::-webkit-search-cancel-button {
    display: none;
  }

  .clear-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.5rem;
    height: 1.5rem;
    border: 0;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--text-muted);
    cursor: pointer;
    flex: 0 0 auto;
    transition: color 0.12s, background 0.12s;
  }

  .clear-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 14%, transparent);
  }

  .busy {
    font-size: max(var(--font-xs, 0.65rem), 0.65rem);
    color: var(--text-muted);
    flex: 0 0 auto;
  }

  .chips {
    display: flex;
    gap: 0.25rem;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .chips button {
    flex: 0 0 auto;
    min-height: 1.8rem;
    padding: 0.2rem 0.6rem;
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    background: var(--surface-1);
    color: var(--text-muted);
    font: inherit;
    font-size: max(var(--font-xs, 0.68rem), 0.68rem);
    cursor: pointer;
    transition: color 0.12s, border-color 0.12s, background 0.12s;
  }

  .chips button:hover:not(.active) {
    color: var(--text-primary);
    border-color: var(--border-emphasis);
  }

  .chips button.active {
    border-color: color-mix(in srgb, var(--color-primary) 45%, transparent);
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 10%, var(--surface-1));
    font-weight: 500;
  }

  button:focus-visible,
  input:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
</style>
