<script lang="ts">
  import { onMount } from 'svelte'
  import type { BrowseScope } from '$lib/types/content'
  import { dockApi } from '$lib/api/dock'
  import { messages } from '$lib/i18n'
  import Icon from '$lib/components/Icon.svelte'

  interface Props {
    currentView: BrowseScope | 'settings'
    onNavigate: (view: BrowseScope) => void
    onToggleSettings: () => void
    onMinimize: () => void
  }

  let { currentView, onNavigate, onToggleSettings, onMinimize }: Props = $props()
  let alwaysOnTop = $state(true)

  onMount(async () => {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      alwaysOnTop = await getCurrentWindow().isAlwaysOnTop()
    } catch {}
  })

  async function togglePin() {
    try {
      alwaysOnTop = await dockApi.toggleAlwaysOnTop()
    } catch {}
  }

  async function drag(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('button')) return
    e.preventDefault()
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      await getCurrentWindow().startDragging()
    } catch {}
  }

  const scopes: [BrowseScope, string][] = $derived([
    ['temporary', messages.workspace.scope.temporary],
    ['all', messages.workspace.scope.all],
    ['saved', messages.workspace.scope.saved],
  ])
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="top" onmousedown={drag}>
  <nav class="scopes">
    {#each scopes as [scope, label]}
      <button
        type="button"
        class="scope-btn"
        class:active={currentView === scope}
        aria-current={currentView === scope ? 'page' : undefined}
        onclick={() => onNavigate(scope)}
      >{label}</button>
    {/each}
  </nav>
  <span class="spacer"></span>
  <button
    type="button"
    class="win-btn"
    class:active={alwaysOnTop}
    aria-label={alwaysOnTop ? messages.nav.unpin : messages.nav.pin}
    title={alwaysOnTop ? messages.nav.unpin : messages.nav.pin}
    onclick={togglePin}
  ><Icon name="pin" size={13} /></button>
  <button
    type="button"
    class="win-btn text-btn"
    class:active={currentView === 'settings'}
    onclick={onToggleSettings}
  >{messages.nav.settings}</button>
  <button
    type="button"
    class="win-btn"
    aria-label={messages.nav.minimize}
    title={messages.nav.minimize}
    onclick={onMinimize}
  ><Icon name="minus" size={13} /></button>
</div>

<style>
  .top {
    min-height: 2.6rem;
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.25rem 0.35rem;
    border-bottom: 1px solid var(--border-subtle);
    cursor: move;
    user-select: none;
  }

  .scopes {
    flex: 0 1 auto;
    min-width: 0;
    display: flex;
    gap: 0.125rem;
    padding: 0.125rem;
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--text-primary) 5%, transparent);
  }

  .scope-btn {
    flex: 0 0 auto;
    min-width: 0;
    min-height: 1.75rem;
    padding: 0.2rem 0.6rem;
    border: 1px solid transparent;
    border-radius: calc(var(--radius-md, 0.35rem) - 0.1rem);
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: max(var(--font-sm, 0.72rem), 0.72rem);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
    transition:
      color var(--dur-fast, 120ms) var(--ease-out, ease-out),
      background var(--dur-fast, 120ms) var(--ease-out, ease-out),
      border-color var(--dur-fast, 120ms) var(--ease-out, ease-out),
      transform var(--dur-fast, 120ms) var(--ease-out, ease-out);
  }

  .scope-btn:hover:not(.active) {
    color: var(--text-primary);
  }

  .scope-btn:active:not(.disabled) {
    transform: scale(0.96);
  }

  .scope-btn.active {
    background: color-mix(in srgb, var(--color-primary) 16%, var(--surface-1));
    border-color: color-mix(in srgb, var(--color-primary) 30%, transparent);
    color: var(--text-primary);
    font-weight: 500;
    box-shadow: 0 1px 4px color-mix(in srgb, var(--color-primary) 18%, transparent);
  }

  /* 分段控制器与窗口按钮之间的拖动空隙（.top 的 mousedown 已处理拖动，此处只是提供可抓取区域） */
  .spacer {
    flex: 1 1 auto;
    align-self: stretch;
    min-width: 0.75rem;
    cursor: move;
  }

  .win-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 1.75rem;
    height: 1.75rem;
    padding: 0 0.3rem;
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: max(var(--font-sm, 0.72rem), 0.72rem);
    white-space: nowrap;
    cursor: pointer;
    transition:
      color var(--dur-fast, 120ms) var(--ease-out, ease-out),
      background var(--dur-fast, 120ms) var(--ease-out, ease-out),
      border-color var(--dur-fast, 120ms) var(--ease-out, ease-out),
      transform var(--dur-fast, 120ms) var(--ease-out, ease-out);
  }

  .win-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
  }

  .win-btn:active {
    transform: scale(0.94);
  }

  .win-btn.active {
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-primary) 30%, transparent);
  }

  .text-btn {
    padding: 0 0.55rem;
  }

  .top button:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 1px;
  }

  @media (max-width: 300px) {
    .top {
      padding: 0.15rem;
      gap: 0.15rem;
    }
    .scope-btn {
      padding: 0.2rem 0.25rem;
    }
    .text-btn {
      padding: 0 0.35rem;
    }
  }
</style>
