<script lang="ts">
  import { messages } from '$lib/i18n'

  interface Props {
    onOpen: () => void | Promise<void>
    disabled?: boolean
  }

  let { onOpen, disabled = false }: Props = $props()

  function handleClick() {
    if (disabled) return
    void onOpen()
  }
</script>

<button
  type="button"
  class="quick-access-fab"
  onclick={handleClick}
  {disabled}
  aria-label={messages.library.openQuickAccess}
  title={messages.library.openQuickAccess}
>
  <svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
    <path d="M13 2 5 14h6l-1 8 8-12h-6l1-8Z" stroke-linejoin="round" />
  </svg>
</button>

<style>
  .quick-access-fab {
    position: absolute;
    right: 0.8rem;
    bottom: 0.8rem;
    z-index: 90;
    width: 3rem;
    height: 3rem;
    display: grid;
    place-items: center;
    padding: 0;
    border: 1px solid color-mix(in srgb, var(--color-primary) 65%, transparent);
    border-radius: 50%;
    background: color-mix(in srgb, var(--color-primary) 82%, var(--surface-0));
    color: var(--surface-0);
    box-shadow: 0 0.45rem 1.2rem color-mix(in srgb, var(--color-primary) 30%, transparent);
    cursor: pointer;
    transition: transform 0.14s ease, box-shadow 0.14s ease, filter 0.14s ease;
  }

  .quick-access-fab svg {
    width: 1.25rem;
    height: 1.25rem;
  }

  .quick-access-fab:hover:not(:disabled) {
    transform: translateY(-2px);
    filter: brightness(1.08);
  }

  .quick-access-fab:active:not(:disabled) {
    transform: translateY(0) scale(0.96);
  }

  .quick-access-fab:focus-visible {
    outline: 2px solid var(--text-primary);
    outline-offset: 3px;
  }

  .quick-access-fab:disabled {
    cursor: wait;
    opacity: 0.65;
  }

  @media (prefers-reduced-motion: reduce) {
    .quick-access-fab {
      transition: none;
    }
  }
</style>
