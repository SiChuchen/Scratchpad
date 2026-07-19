<script lang="ts">
  import { messages } from '$lib/i18n'

  interface Props {
    pinned: boolean
    pinPending?: boolean
    onTogglePin: () => void | Promise<void>
    onHide: () => void | Promise<void>
    onDrag: () => void | Promise<void>
  }

  let {
    pinned,
    pinPending = false,
    onTogglePin,
    onHide,
    onDrag,
  }: Props = $props()

  function handleMouseDown(event: MouseEvent) {
    if (event.button !== 0) return
    if ((event.target as HTMLElement).closest('button')) return
    event.preventDefault()
    void onDrag()
  }

  function handleTogglePin() {
    if (pinPending) return
    void onTogglePin()
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="window-bar"
  data-testid="quick-access-drag-region"
  onmousedown={handleMouseDown}
>
  <span class="accent" aria-hidden="true"></span>
  <span class="title">{messages.library.quickAccess}</span>
  <span class="spacer"></span>
  <button
    type="button"
    class="bar-button pin"
    class:active={pinned}
    disabled={pinPending}
    aria-pressed={pinned}
    aria-label={pinned ? messages.nav.unpin : messages.nav.pin}
    title={pinned ? messages.nav.unpin : messages.nav.pin}
    onclick={handleTogglePin}
  >
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      fill={pinned ? 'currentColor' : 'none'}
      stroke="currentColor"
      stroke-width="1.7"
      stroke-linejoin="round"
    >
      <path d="M12 17v5M9 3h6l-1 7h3l-5 7-5-7h3z" />
    </svg>
  </button>
  <button
    type="button"
    class="bar-button close"
    aria-label={messages.quickAccess.close}
    title={messages.quickAccess.close}
    onclick={() => void onHide()}
  >
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.8"
      stroke-linecap="round"
    >
      <path d="m7 7 10 10M17 7 7 17" />
    </svg>
  </button>
</div>

<style>
  .window-bar {
    min-height: 2.15rem;
    display: flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.15rem 0.3rem 0.15rem 0.65rem;
    border-bottom: 1px solid var(--border-default);
    background: color-mix(in srgb, var(--surface-0) 92%, var(--surface-1));
    color: var(--text-muted);
    user-select: none;
    cursor: move;
  }

  .accent {
    width: 0.42rem;
    height: 0.42rem;
    flex: 0 0 auto;
    border-radius: 50%;
    background: var(--color-primary);
    box-shadow: 0 0 0.5rem color-mix(in srgb, var(--color-primary) 55%, transparent);
  }

  .title {
    font-size: var(--font-sm, 0.75rem);
    font-weight: 650;
    color: var(--text-secondary, var(--text-primary));
  }

  .spacer {
    flex: 1;
  }

  .bar-button {
    width: 1.9rem;
    height: 1.9rem;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--radius-md, 0.35rem);
    background: transparent;
    color: var(--text-muted);
    font: inherit;
    cursor: pointer;
    transition: color 0.12s ease, background 0.12s ease, border-color 0.12s ease;
  }

  .bar-button svg {
    width: 0.9rem;
    height: 0.9rem;
  }

  .bar-button:hover:not(:disabled),
  .bar-button:focus-visible {
    color: var(--text-primary);
    border-color: var(--border-emphasis);
    background: var(--surface-2);
  }

  .bar-button:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 1px;
  }

  .bar-button.pin.active {
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
  }

  .bar-button.close:hover,
  .bar-button.close:focus-visible {
    color: var(--color-danger);
    border-color: color-mix(in srgb, var(--color-danger) 35%, transparent);
    background: color-mix(in srgb, var(--color-danger) 11%, transparent);
  }

  .bar-button:disabled {
    opacity: 0.55;
    cursor: wait;
  }

  @media (prefers-reduced-motion: reduce) {
    .bar-button {
      transition: none;
    }
  }
</style>
