<script lang="ts">
  interface Props {
    onRestore: () => void
    onPeek: () => void
    onHide: () => void
  }

  let { onRestore, onPeek, onHide }: Props = $props()

  let dragging = $state(false)
  let didMove = $state(false)
  let dragStart = { x: 0, y: 0 }
  let winStart = { x: 0, y: 0 }

  async function handleMouseDown(e: MouseEvent) {
    e.preventDefault()
    dragging = true
    didMove = false
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      const win = getCurrentWindow()
      const pos = await win.outerPosition()
      dragStart = { x: e.screenX, y: e.screenY }
      winStart = { x: pos.x, y: pos.y }
    } catch {}
  }

  async function handleMouseMove(e: MouseEvent) {
    if (!dragging) return
    const dx = e.screenX - dragStart.x
    const dy = e.screenY - dragStart.y
    if (Math.abs(dx) > 3 || Math.abs(dy) > 3) didMove = true
    try {
      const { getCurrentWindow, LogicalPosition } = await import('@tauri-apps/api/window')
      await getCurrentWindow().setPosition(new LogicalPosition(winStart.x + dx, winStart.y + dy))
    } catch {}
  }

  async function handleMouseUp() {
    if (!dragging) return
    dragging = false
    if (didMove) {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window')
        const win = getCurrentWindow()
        const pos = await win.outerPosition()
        localStorage.setItem('dock-minimized-tab', JSON.stringify({ x: pos.x, y: pos.y }))
      } catch {}
    } else {
      onRestore()
    }
  }
</script>

<svelte:window onmousemove={handleMouseMove} onmouseup={handleMouseUp} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="minimized-tab"
  onmousedown={handleMouseDown}
  onmouseenter={onPeek}
  onmouseleave={dragging ? undefined : onHide}
  role="button"
  tabindex="0"
>
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
    <rect x="3" y="3" width="18" height="18" rx="3" />
    <line x1="9" y1="3" x2="9" y2="21" />
  </svg>
</div>

<style>
  .minimized-tab {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--dock-bg-color, #0f172a) 88%, transparent);
    backdrop-filter: blur(12px);
    border: 1px solid color-mix(in srgb, var(--dock-text-color, #e5eef7) 20%, transparent);
    border-radius: 0.5rem;
    color: color-mix(in srgb, var(--dock-text-color, #e5eef7) 70%, transparent);
    cursor: default;
    font-family: inherit;
    transition: background 0.15s, border-color 0.15s, color 0.15s;
    box-shadow: 0 2px 12px rgba(0, 0, 0, 0.25);
  }

  .minimized-tab:hover {
    background: color-mix(in srgb, var(--dock-bg-color, #0f172a) 95%, transparent);
    border-color: color-mix(in srgb, var(--dock-text-color, #e5eef7) 40%, transparent);
    color: var(--dock-text-color, #e5eef7);
    box-shadow: 0 2px 16px rgba(0, 0, 0, 0.35);
  }

  .minimized-tab:active {
    cursor: default;
  }
</style>
