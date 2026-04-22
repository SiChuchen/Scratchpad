<script lang="ts">
  import { onMount } from 'svelte'

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
  <img src="/app-icon-circle.png" alt="" class="tab-icon" draggable="false" />
</div>

<style>
  .minimized-tab {
    width: 100%;
    height: 100%;
    background: transparent;
    border: none;
    margin: 0;
    padding: 0;
    cursor: default;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .tab-icon {
    width: 100%;
    height: 100%;
    object-fit: contain;
    pointer-events: none;
    transition: transform 0.15s ease, filter 0.15s ease;
  }

  .minimized-tab:hover .tab-icon {
    transform: scale(1.1);
    filter: brightness(1.1);
  }
</style>
