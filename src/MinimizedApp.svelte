<script lang="ts">
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'

  const win = getCurrentWindow()
  let hideTimer: ReturnType<typeof setTimeout> | null = null

  onMount(() => {
    scheduleAutoHide()
  })

  function handleMouseEnter() {
    if (hideTimer) clearTimeout(hideTimer)
    ;(win as any).setOpacity(1).catch((e: unknown) => console.error('setOpacity failed:', e))
  }

  function handleMouseLeave() {
    scheduleAutoHide()
  }

  function scheduleAutoHide() {
    if (hideTimer) clearTimeout(hideTimer)
    hideTimer = setTimeout(() => {
      ;(win as any).setOpacity(0.35).catch((e: unknown) => console.error('setOpacity failed:', e))
    }, 2500)
  }
</script>

<div
  class="minimized-tab"
  onmouseenter={handleMouseEnter}
  onmouseleave={handleMouseLeave}
>
  <img src="/app-icon-circle.png" alt="" class="tab-icon" draggable="false" />
</div>

<style>
  .minimized-tab {
    width: 48px;
    height: 48px;
    border-radius: 50%;
    overflow: hidden;
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
    object-fit: cover;
    border-radius: 50%;
    pointer-events: none;
    transition: transform 0.15s ease, filter 0.15s ease;
  }

  .minimized-tab:hover .tab-icon {
    transform: scale(1.1);
    filter: brightness(1.1);
  }
</style>
