<script lang="ts">
  import type { DockView } from '$lib/types/dock'
  import { dockApi } from '$lib/api/dock'
  import { messages } from '$lib/i18n'

  interface Props {
    currentView: DockView
    onNavigate: (view: DockView) => void
    onToggleSettings: () => void
    onMinimize: () => void
  }

  let { currentView, onNavigate, onToggleSettings, onMinimize }: Props = $props()
  let alwaysOnTop = $state(true)

  async function togglePin() {
    try {
      alwaysOnTop = await dockApi.toggleAlwaysOnTop()
    } catch {}
  }

  async function handleMouseDown(event: MouseEvent) {
    const target = event.target as HTMLElement
    if (target.closest('button')) return
    event.preventDefault()
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      await getCurrentWindow().startDragging()
    } catch {}
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="top-bar" onmousedown={handleMouseDown}>
  <button
    class="nav-btn"
    class:active={currentView === 'home'}
    onclick={() => onNavigate('home')}
  >{messages.nav.home}</button>
  <button
    class="nav-btn"
    class:active={currentView === 'categories'}
    onclick={() => onNavigate('categories')}
  >{messages.nav.all}</button>
  <button
    class="nav-btn"
    class:active={currentView === 'note'}
    onclick={() => onNavigate('note')}
  >{messages.nav.favorites}</button>
  <div class="top-bar-spacer"></div>
  <button
    class="nav-btn pin-btn"
    class:active={alwaysOnTop}
    onclick={togglePin}
    title={alwaysOnTop ? messages.nav.unpin : messages.nav.pin}
  >
    <svg width="11" height="11" viewBox="0 0 24 24" fill={alwaysOnTop ? 'currentColor' : 'none'} stroke="currentColor" stroke-width="1.5">
      <path d="M12 17v5M9 3h6l-1 7h3l-5 7-5-7h3z" />
    </svg>
  </button>
  <button
    class="nav-btn"
    class:active={currentView === 'settings'}
    onclick={onToggleSettings}
  >{messages.nav.settings}</button>
  <button class="nav-btn minimize-btn" onclick={onMinimize} title={messages.nav.minimize}>
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  </button>
</div>

<style>
  .top-bar {
    display: flex;
    align-items: center;
    gap: 0.1rem;
    padding: 0.25rem 0.4rem;
    flex-shrink: 0;
    user-select: none;
    border-bottom: 1px solid var(--border-subtle);
    cursor: move;
  }

  .nav-btn {
    background: none;
    border: 1px solid transparent;
    color: color-mix(in srgb, var(--text-primary) 50%, transparent);
    font-size: var(--font-sm, 0.65rem);
    font-weight: 500;
    padding: 0.25rem 0.5rem;
    border-radius: var(--radius-md, 0.3rem);
    cursor: pointer;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
    font-family: inherit;
    white-space: nowrap;
  }

  .nav-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
    border-color: color-mix(in srgb, var(--text-primary) 15%, transparent);
  }

  .nav-btn.active {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 15%, transparent);
    border-color: color-mix(in srgb, var(--text-primary) 25%, transparent);
  }

  .top-bar-spacer {
    flex: 1;
    cursor: move;
  }

  .minimize-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.6rem;
    height: 1.6rem;
    padding: 0;
    border: 1px solid transparent;
  }

  .pin-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.6rem;
    height: 1.6rem;
    padding: 0;
  }

  .pin-btn.active {
    color: var(--color-primary);
  }
</style>
