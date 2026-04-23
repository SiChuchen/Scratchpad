<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import TopBar from '$lib/components/TopBar.svelte'
  import HomeView from '$lib/components/views/HomeView.svelte'
  import CategoriesView from '$lib/components/views/CategoriesView.svelte'
  import NoteView from '$lib/components/views/NoteView.svelte'
  import SettingsView from '$lib/components/views/SettingsView.svelte'
  import MinimizedTab from '$lib/components/MinimizedTab.svelte'
  import { dockApi } from '$lib/api/dock'
  import { insertHomeEntry, removeEntryFromView } from '$lib/state/dock'
  import { anchorToNearestEdge, hiddenTabRect } from '$lib/state/window'
  import type { DockEntry, DockPreferences, DockView } from '$lib/types/dock'

  let currentView = $state<DockView>('home')
  let dockMode = $state<'expanded' | 'minimized'>('expanded')
  let homeEntries = $state<DockEntry[]>([])
  let noteEntries = $state<DockEntry[]>([])
  let preferences = $state<DockPreferences | null>(null)
  let toast = $state<{ text: string; kind: 'success' | 'error'; undo?: () => void } | null>(null)
  let toastTimer: ReturnType<typeof setTimeout> | null = null

  onMount(async () => {
    try {
      ;[homeEntries, noteEntries, preferences] = await Promise.all([
        dockApi.listEntries('home'),
        dockApi.listEntries('note'),
        dockApi.getPreferences(),
      ])
    } catch (e) {
      showToast(`加载失败: ${formatError(e)}`, 'error')
    }

    window.addEventListener('paste', handleGlobalPaste as unknown as EventListener)

    // Tauri native drag-drop for file imports
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    const win = getCurrentWindow()
    const unlisten = await win.onDragDropEvent((event) => {
      if (event.payload.type === 'drop') {
        handleNativeFileDrop(event.payload.paths)
        // Reclaim focus after OS-level drag-drop steals it
        win.setFocus()
      }
    })
  })

  // Apply preferences as CSS variables
  $effect(() => {
    if (!preferences) return
    const root = document.documentElement.style
    root.setProperty('--dock-bg-opacity', `${preferences.dockBgOpacity * 100}%`)
    root.setProperty('--dock-bg-color', preferences.dockBgColor)
    root.setProperty('--dock-text-size', `${preferences.textSizePx}px`)
    root.setProperty('--dock-text-color', preferences.textColor)
    root.setProperty('--dock-font-zh', preferences.fontFamilyZh)
    root.setProperty('--dock-font-en', preferences.fontFamilyEn)
    root.setProperty('--entry-surface-opacity', String(preferences.entrySurfaceOpacity))
  })

  // --- Home handlers ---

  async function createHomeText(content: string) {
    try {
      const created = await dockApi.createText('home', content, 'manual')
      homeEntries = insertHomeEntry(homeEntries, created)
    } catch (e) {
      showToast(`创建失败: ${formatError(e)}`, 'error')
    }
  }

  function importHomeEntry(entry: DockEntry) {
    homeEntries = insertHomeEntry(homeEntries, { ...entry, inHome: true })
  }

  async function toggleCollapse(entryId: string) {
    const entry = [...homeEntries, ...noteEntries].find((e) => e.id === entryId)
    if (!entry) return
    const newCollapsed = !entry.collapsed
    try {
      await dockApi.toggleCollapse(entryId, newCollapsed)
      homeEntries = homeEntries.map((e) =>
        e.id === entryId ? { ...e, collapsed: newCollapsed } : e,
      )
      noteEntries = noteEntries.map((e) =>
        e.id === entryId ? { ...e, collapsed: newCollapsed } : e,
      )
    } catch (e) {
      showToast(`操作失败: ${formatError(e)}`, 'error')
    }
  }

  async function deleteFromView(view: 'home' | 'note', entryId: string) {
    const list = view === 'home' ? homeEntries : noteEntries
    const entry = list.find((e) => e.id === entryId)
    if (!entry) return
    const entryIdx = list.indexOf(entry)

    // Optimistically remove from UI
    if (view === 'home') {
      homeEntries = homeEntries.filter((e) => e.id !== entryId)
    } else {
      noteEntries = noteEntries.filter((e) => e.id !== entryId)
    }

    let committed = false
    const commitDelete = async () => {
      if (committed) return
      committed = true
      try {
        await dockApi.removeFromView(view, entryId)
      } catch (e) {
        showToast(`删除失败: ${formatError(e)}`, 'error')
      }
    }

    const undoDelete = () => {
      if (committed) return
      // Re-insert at original position
      if (view === 'home') {
        const next = [...homeEntries]
        next.splice(entryIdx, 0, entry)
        homeEntries = next
      } else {
        const next = [...noteEntries]
        next.splice(entryIdx, 0, entry)
        noteEntries = next
      }
      toast = null
      if (toastTimer) clearTimeout(toastTimer)
    }

    // Show toast with undo, auto-commit after 3s
    showToast('已删除', 'success', undoDelete)
    toastTimer = setTimeout(() => {
      commitDelete()
    }, 3000)
  }

  async function addToNote(entryId: string) {
    try {
      await dockApi.addToNote(entryId)
      homeEntries = homeEntries.map((e) =>
        e.id === entryId ? { ...e, inNote: true } : e,
      )
      noteEntries = noteEntries.map((e) =>
        e.id === entryId ? { ...e, inNote: true } : e,
      )
      // Refresh note entries to include the new one
      noteEntries = await dockApi.listEntries('note')
      showToast('已收藏到 Note')
    } catch (e) {
      showToast(`收藏失败: ${formatError(e)}`, 'error')
    }
  }

  async function updateText(id: string, content: string) {
    try {
      await dockApi.updateText(id, content)
      homeEntries = homeEntries.map((e) =>
        e.id === id ? { ...e, content } : e,
      )
      noteEntries = noteEntries.map((e) =>
        e.id === id ? { ...e, content } : e,
      )
    } catch (e) {
      showToast(`编辑失败: ${formatError(e)}`, 'error')
    }
  }

  async function copyContent(content: string) {
    try {
      await navigator.clipboard.writeText(content)
      showToast('已复制')
    } catch {
      showToast('复制失败', 'error')
    }
  }

  async function copyPath(path: string) {
    try {
      await navigator.clipboard.writeText(path)
      showToast('已复制路径')
    } catch {
      showToast('复制失败', 'error')
    }
  }

  // --- Note handlers ---

  async function createNoteText(content: string) {
    try {
      const created = await dockApi.createText('note', content, 'manual')
      noteEntries = [{ ...created, inNote: true, inHome: false }, ...noteEntries]
    } catch (e) {
      showToast(`创建失败: ${formatError(e)}`, 'error')
    }
  }

  function importNoteEntry(entry: DockEntry) {
    noteEntries = [{ ...entry, inNote: true, inHome: false }, ...noteEntries]
  }

  // --- Preferences ---

  async function updatePreferences(next: DockPreferences) {
    try {
      await dockApi.setPreferences(next)
      preferences = next

      // Sync autostart with OS
      try {
        const { enable, disable, isEnabled } = await import('@tauri-apps/plugin-autostart')
        if (next.launchOnStartup) {
          if (!(await isEnabled())) await enable()
        } else {
          if (await isEnabled()) await disable()
        }
      } catch {
        // autostart plugin may not be available in dev
      }
    } catch (e) {
      showToast(`保存失败: ${formatError(e)}`, 'error')
    }
  }

  // --- Merged entries for categories view ---
  let allEntries = $derived.by(() => {
    const seen = new Set<string>()
    const result: DockEntry[] = []
    for (const e of [...homeEntries, ...noteEntries]) {
      if (!seen.has(e.id)) {
        seen.add(e.id)
        result.push(e)
      }
    }
    return result
  })

  function deleteFromAnyView(entryId: string) {
    const entry = allEntries.find((e) => e.id === entryId)
    if (!entry) return
    if (entry.inHome) deleteFromView('home', entryId)
    if (entry.inNote) deleteFromView('note', entryId)
  }

  // --- Navigation ---

  let prevView: DockView = 'home'

  function navigate(view: DockView) {
    if (view === 'settings' && currentView !== 'settings') {
      prevView = currentView
    }
    currentView = view
  }

  function toggleSettings() {
    if (currentView === 'settings') {
      currentView = prevView
    } else {
      prevView = currentView
      currentView = 'settings'
    }
  }

  let hideTimer: ReturnType<typeof setTimeout> | null = null

  async function minimize() {
    if (!preferences) return
    const { getCurrentWindow, currentMonitor, LogicalSize, LogicalPosition } = await import('@tauri-apps/api/window')
    const win = getCurrentWindow()
    const monitor = await currentMonitor()
    if (!monitor) return

    dockMode = 'minimized'
    // Force all backgrounds transparent for circular minimized look
    document.documentElement.style.background = 'transparent'
    document.body.style.background = 'transparent'
    document.body.style.minWidth = '0'
    const appEl = document.getElementById('app')!
    appEl.style.background = 'transparent'
    appEl.style.backdropFilter = 'none'
    appEl.style.border = 'none'
    appEl.style.boxShadow = 'none'
    appEl.style.borderRadius = '0'
    appEl.style.overflow = 'hidden'
    const tabSize = { width: 48, height: 48 }
    const winRect = { x: preferences.dockPositionX, y: preferences.dockPositionY, width: preferences.dockWidth, height: preferences.dockHeight }
    const screen = { width: monitor.size.width, height: monitor.size.height }
    const anchor = anchorToNearestEdge(winRect, screen)
    const winCenter = {
      x: winRect.x + Math.round(winRect.width / 2),
      y: winRect.y + Math.round(winRect.height / 2),
    }

    const rect = hiddenTabRect(anchor, tabSize, screen, winCenter)

    await win.setSize(new LogicalSize(rect.width, rect.height))
    await win.setPosition(new LogicalPosition(rect.x, rect.y))
    scheduleAutoHide()
  }

  async function restoreFromMinimized() {
    if (!preferences) return
    if (hideTimer) clearTimeout(hideTimer)
    const { getCurrentWindow, LogicalSize, LogicalPosition } = await import('@tauri-apps/api/window')
    const win = getCurrentWindow()

    dockMode = 'expanded'
    // Restore normal styles
    document.documentElement.style.background = ''
    document.body.style.background = ''
    document.body.style.minWidth = ''
    const appEl = document.getElementById('app')!
    appEl.style.background = ''
    appEl.style.backdropFilter = ''
    appEl.style.border = ''
    appEl.style.boxShadow = ''
    appEl.style.borderRadius = ''
    appEl.style.overflow = ''
    await win.setSize(new LogicalSize(preferences.dockWidth, preferences.dockHeight))
    await win.setPosition(new LogicalPosition(preferences.dockPositionX, preferences.dockPositionY))
    try { await (win as any).setOpacity(1) } catch {}
  }

  async function peekTab() {
    if (hideTimer) clearTimeout(hideTimer)
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    try { await (getCurrentWindow() as any).setOpacity(1) } catch {}
  }

  function scheduleAutoHide() {
    if (hideTimer) clearTimeout(hideTimer)
    hideTimer = setTimeout(async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      try { await (getCurrentWindow() as any).setOpacity(0.35) } catch {}
    }, 2500)
  }

  // --- Ctrl+click drag ---

  let ctrlHeld = $state(false)

  let pasteConsumed = false

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Control') ctrlHeld = true
    if (e.ctrlKey && e.code === 'KeyV' && dockMode !== 'minimized') {
      const target = e.target as HTMLElement
      if (target.closest('textarea, input')) return
      pasteConsumed = false
      setTimeout(() => {
        if (!pasteConsumed) handleClipboardTextFallback()
      }, 100)
    }
  }

  function handleKeyUp(e: KeyboardEvent) {
    if (e.key === 'Control') ctrlHeld = false
  }

  // --- Global paste ---

  async function handleGlobalPaste(event: ClipboardEvent) {
    if (dockMode === 'minimized') return
    const target = event.target as HTMLElement
    const inEditor = !!target.closest('textarea, input')

    const items = Array.from(event.clipboardData?.items ?? [])
    const files = Array.from(event.clipboardData?.files ?? [])
    const imageItem = items.find((item) => item.type.startsWith('image/'))
    const pastedText = event.clipboardData?.getData('text/plain')?.trim()

    // Always handle images and files regardless of focus target
    if (imageItem) {
      event.preventDefault()
      pasteConsumed = true
      const blob = imageItem.getAsFile()
      if (!blob) return
      try {
        const view = currentView === 'note' ? 'note' : 'home'
        const created = await dockApi.importImageBlob(blob, `pasted-${Date.now()}.png`, view)
        if (view === 'note') {
          noteEntries = [{ ...created, inNote: true, inHome: false }, ...noteEntries]
        } else {
          homeEntries = insertHomeEntry(homeEntries, { ...created, inHome: true })
        }
      } catch (e) {
        showToast(`粘贴失败: ${formatError(e)}`, 'error')
      }
      return
    }

    // Handle files copied from Explorer (Ctrl+C file → Ctrl+V here)
    if (files.length > 0) {
      event.preventDefault()
      pasteConsumed = true
      try {
        const view = currentView === 'note' ? 'note' : 'home'
        for (const file of files) {
          if (file.type.startsWith('image/')) {
            const created = await dockApi.importImageBlob(file, file.name || `pasted-${Date.now()}.png`, view)
            if (view === 'note') {
              noteEntries = [{ ...created, inNote: true, inHome: false }, ...noteEntries]
            } else {
              homeEntries = insertHomeEntry(homeEntries, { ...created, inHome: true })
            }
          } else {
            const bytes = Array.from(new Uint8Array(await file.arrayBuffer()))
            const created = await invoke<DockEntry>('ipc_entries_import_file_bytes', {
              bytes,
              fileName: file.name || `file-${Date.now()}`,
              mimeType: file.type || null,
              view,
            })
            if (view === 'note') {
              noteEntries = [{ ...created, inNote: true, inHome: false }, ...noteEntries]
            } else {
              homeEntries = insertHomeEntry(homeEntries, { ...created, inHome: true })
            }
          }
        }
        showToast(`已导入 ${files.length} 个文件`)
      } catch (e) {
        showToast(`导入失败: ${formatError(e)}`, 'error')
      }
      return
    }

    // Only create text entries if not pasting into a textarea/input (normal input behavior)
    if (pastedText && !inEditor) {
      event.preventDefault()
      pasteConsumed = true
      try {
        if (currentView === 'note') {
          const created = await dockApi.createText('note', pastedText, 'manual')
          noteEntries = [{ ...created, inNote: true, inHome: false }, ...noteEntries]
        } else {
          const created = await dockApi.createText('home', pastedText, 'manual')
          homeEntries = insertHomeEntry(homeEntries, created)
        }
      } catch (e) {
        showToast(`粘贴失败: ${formatError(e)}`, 'error')
      }
    }
  }

  async function handleClipboardTextFallback() {
    try {
      const text = await navigator.clipboard.readText()
      if (text?.trim()) {
        if (currentView === 'note') {
          const created = await dockApi.createText('note', text, 'manual')
          noteEntries = [{ ...created, inNote: true, inHome: false }, ...noteEntries]
        } else {
          const created = await dockApi.createText('home', text, 'manual')
          homeEntries = insertHomeEntry(homeEntries, created)
        }
        showToast('已粘贴')
      }
    } catch {
      showToast('粘贴失败', 'error')
    }
  }

  async function handleNativeFileDrop(paths: string[]) {
    if (dockMode === 'minimized') return
    try {
      const view: 'home' | 'note' = currentView === 'note' ? 'note' : 'home'
      for (const path of paths) {
        const created = await dockApi.importFile(path, view)
        if (view === 'note') {
          noteEntries = [{ ...created, inNote: true, inHome: false }, ...noteEntries]
        } else {
          homeEntries = insertHomeEntry(homeEntries, { ...created, inHome: true })
        }
      }
    } catch (e) {
      showToast(`导入失败: ${formatError(e)}`, 'error')
    }
  }


  async function handleAppPointerDown(event: MouseEvent) {
    if (!event.ctrlKey) return
    event.preventDefault()
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      await getCurrentWindow().startDragging()
    } catch {}
  }

  function handleGlobalDragStart(event: DragEvent) {
    if (ctrlHeld) {
      event.preventDefault()
    }
  }

  // --- Toast ---

  function showToast(text: string, kind: 'success' | 'error' = 'success', undo?: () => void) {
    if (toastTimer) clearTimeout(toastTimer)
    toast = { text, kind, undo }
    toastTimer = setTimeout(() => {
      toast = null
    }, 3000)
  }

  function formatError(error: unknown): string {
    if (error instanceof Error && error.message) return error.message
    if (typeof error === 'string') return error
    return '未知错误'
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<svelte:window onkeydown={handleKeyDown} onkeyup={handleKeyUp} ondragstart={handleGlobalDragStart} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="app-shell" class:ctrl-drag={ctrlHeld} onmousedown={handleAppPointerDown}>
{#if dockMode === 'minimized'}
  <MinimizedTab onRestore={restoreFromMinimized} onPeek={peekTab} onHide={scheduleAutoHide} />
{:else}
  <TopBar {currentView} onNavigate={navigate} onToggleSettings={toggleSettings} onMinimize={minimize} />

  {#if currentView === 'home'}
  <HomeView
    entries={homeEntries}
    onToggleCollapse={toggleCollapse}
    onDeleteFromView={(id) => deleteFromView('home', id)}
    onAddToNote={addToNote}
    onCreateText={createHomeText}
    onImportEntry={importHomeEntry}
    onUpdateText={updateText}
    onCopy={copyContent}
    onCopyPath={copyPath}
  />
{:else if currentView === 'categories'}
  <CategoriesView
    entries={allEntries}
    onToggleCollapse={toggleCollapse}
    onDeleteFromView={deleteFromAnyView}
    onAddToNote={addToNote}
    onUpdateText={updateText}
    onCopy={copyContent}
    onCopyPath={copyPath}
  />
{:else if currentView === 'note'}
  <NoteView
    entries={noteEntries}
    onToggleCollapse={toggleCollapse}
    onDeleteFromView={(id) => deleteFromView('note', id)}
    onCreateText={createNoteText}
    onImportEntry={importNoteEntry}
    onUpdateText={updateText}
    onCopy={copyContent}
    onCopyPath={copyPath}
  />
{:else if currentView === 'settings' && preferences}
  <SettingsView
    preferences={preferences}
    onChange={updatePreferences}
    onBack={() => navigate('home')}
  />
{/if}
{/if}
</div>

{#if toast}
  <div class="toast" class:toast-error={toast.kind === 'error'}>
    <span>{toast.text}</span>
    {#if toast.undo}
      <button class="toast-undo" onclick={toast.undo}>撤销</button>
    {/if}
  </div>
{/if}

<style>
  .app-shell {
    height: 100vh;
    display: flex;
    flex-direction: column;
  }

  .ctrl-drag {
    cursor: move !important;
  }

  .ctrl-drag :global(*) {
    cursor: move !important;
  }

  .toast {
    position: absolute;
    bottom: 0.75rem;
    left: 50%;
    transform: translateX(-50%);
    background: rgba(15, 23, 42, 0.92);
    border: 1px solid rgba(125, 211, 252, 0.3);
    color: rgba(125, 211, 252, 0.9);
    padding: 0.35rem 0.6rem;
    border-radius: 0.5rem;
    font-size: 0.7rem;
    white-space: nowrap;
    z-index: 110;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    animation: toast-in 0.2s ease-out;
  }

  .toast-undo {
    background: none;
    border: none;
    color: rgba(125, 211, 252, 1);
    font-weight: 600;
    font-size: 0.7rem;
    cursor: pointer;
    padding: 0;
    font-family: inherit;
    text-decoration: underline;
  }

  .toast-undo:hover {
    color: rgba(147, 197, 253, 1);
  }

  .toast-error {
    border-color: rgba(248, 113, 113, 0.3);
    color: rgba(248, 113, 113, 0.9);
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(8px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }
</style>
