<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import TopBar from '$lib/components/TopBar.svelte'
  import HomeView from '$lib/components/views/HomeView.svelte'
  import CategoriesView from '$lib/components/views/CategoriesView.svelte'
  import NoteView from '$lib/components/views/NoteView.svelte'
  import SettingsView from '$lib/components/views/SettingsView.svelte'
  import { dockApi } from '$lib/api/dock'
  import { insertHomeEntry } from '$lib/state/dock'
  import { computeThemeTokens } from '$lib/themes/engine'

  import type { DockEntry, DockPreferences, DockView } from '$lib/types/dock'

  let currentView = $state<DockView>('home')
  let homeEntries = $state<DockEntry[]>([])
  let noteEntries = $state<DockEntry[]>([])
  let preferences = $state<DockPreferences | null>(null)
  let toast = $state<{ text: string; kind: 'success' | 'error'; undo?: () => void; actionLabel?: string } | null>(null)
  let toastTimer: ReturnType<typeof setTimeout> | null = null

  // Reactive system dark mode — separate from main async onMount
  let systemDark = $state(window.matchMedia('(prefers-color-scheme: dark)').matches)

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
    const unlisten = await win.onDragDropEvent((event: any) => {
      if (event.payload.type === 'enter') {
        dragOverlay = { active: true, count: event.payload.paths?.length ?? 1 }
      } else if (event.payload.type === 'drop') {
        dragOverlay = { active: false, count: 0 }
        handleNativeFileDrop(event.payload.paths)
        win.setFocus()
      } else if (event.payload.type === 'leave') {
        dragOverlay = { active: false, count: 0 }
      }
    })

    // Check for updates on startup
    checkForUpdate()
  })

  // Synchronous onMount for matchMedia listener — cleanup function works correctly
  onMount(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    function onSystemThemeChange(e: MediaQueryListEvent) {
      systemDark = e.matches
    }
    mq.addEventListener('change', onSystemThemeChange)
    return () => mq.removeEventListener('change', onSystemThemeChange)
  })

  // --- Update check ---

  async function checkForUpdate() {
    try {
      const { check } = await import('@tauri-apps/plugin-updater')
      const proxy = preferences?.updateProxy?.trim()
      const proxyUrl = proxy ? (proxy.startsWith('http') ? proxy : `http://${proxy}`) : undefined
      const update = await check({ proxy: proxyUrl })
      if (update?.available) {
        showToast(`发现新版本 v${update.version}`, 'success', () => installUpdate(update), '更新')
      }
    } catch {
      // update check is not critical
    }
  }

  async function installUpdate(update: any) {
    showToast('正在下载更新...', 'success')
    try {
      await update.downloadAndInstall()
      showToast('更新完成，即将重启...', 'success')
      setTimeout(async () => {
        const { getCurrentWindow } = await import('@tauri-apps/api/window')
        getCurrentWindow().close()
      }, 1500)
    } catch (e) {
      showToast(`更新失败: ${formatError(e)}`, 'error')
    }
  }

  // Apply theme tokens as CSS variables — reacts to both preferences and systemDark
  $effect(() => {
    if (!preferences) return
    const tokens = computeThemeTokens(preferences, systemDark)
    const root = document.documentElement.style
    for (const [key, value] of Object.entries(tokens)) {
      root.setProperty(key, value)
    }
    // Non-token preferences
    root.setProperty('--font-family-zh', preferences.fontFamilyZh)
    root.setProperty('--font-family-en', preferences.fontFamilyEn)
  })

  // --- Home handlers ---

  const collapsePendingIds = new Set<string>()

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
    if (collapsePendingIds.has(entryId)) return
    if (!entry) return
    const previousCollapsed = entry.collapsed
    const newCollapsed = !entry.collapsed
    collapsePendingIds.add(entryId)
    homeEntries = homeEntries.map((e) =>
      e.id === entryId ? { ...e, collapsed: newCollapsed } : e,
    )
    noteEntries = noteEntries.map((e) =>
      e.id === entryId ? { ...e, collapsed: newCollapsed } : e,
    )
    try {
      await dockApi.toggleCollapse(entryId, newCollapsed)
    } catch (e) {
      homeEntries = homeEntries.map((entry) =>
        entry.id === entryId ? { ...entry, collapsed: previousCollapsed } : entry,
      )
      noteEntries = noteEntries.map((entry) =>
        entry.id === entryId ? { ...entry, collapsed: previousCollapsed } : entry,
      )
      showToast(`操作失败: ${formatError(e)}`, 'error')
    } finally {
      collapsePendingIds.delete(entryId)
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
    showToast('已删除 1 条内容', 'success', undoDelete)
    toastTimer = setTimeout(() => {
      commitDelete()
    }, 3000)
  }

  async function toggleNote(entryId: string) {
    const entry = allEntries.find((e) => e.id === entryId)
    if (!entry) return

    try {
      if (entry.inNote) {
        await dockApi.removeFromView('note', entryId)
        homeEntries = homeEntries.map((e) =>
          e.id === entryId ? { ...e, inNote: false } : e,
        )
        noteEntries = noteEntries.filter((e) => e.id !== entryId)
      } else {
        await dockApi.addToNote(entryId)
        homeEntries = homeEntries.map((e) =>
          e.id === entryId ? { ...e, inNote: true } : e,
        )
        noteEntries = noteEntries.map((e) =>
          e.id === entryId ? { ...e, inNote: true } : e,
        )
        noteEntries = await dockApi.listEntries('note')
      }
    } catch (e) {
      showToast(`收藏操作失败: ${formatError(e)}`, 'error')
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

  async function renameEntry(id: string, title: string | null) {
    try {
      await dockApi.rename(id, title)
      homeEntries = homeEntries.map((e) =>
        e.id === id ? { ...e, title } : e,
      )
      noteEntries = noteEntries.map((e) =>
        e.id === id ? { ...e, title } : e,
      )
    } catch (e) {
      showToast(`重命名失败: ${formatError(e)}`, 'error')
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

  let saveTimer: ReturnType<typeof setTimeout> | null = null

  async function updatePreferences(next: DockPreferences) {
    const prev = preferences
    preferences = next  // immediate visual effect via $effect

    // Detect if system settings changed (need immediate save + OS sync)
    const autostartChanged = prev?.launchOnStartup !== next.launchOnStartup

    if (saveTimer) { clearTimeout(saveTimer); saveTimer = null }

    if (autostartChanged) {
      // Immediate save for system settings
      try {
        await dockApi.setPreferences(next)
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
    } else {
      // Debounce appearance/theme changes (300ms)
      saveTimer = setTimeout(async () => {
        try {
          await dockApi.setPreferences(preferences!)
        } catch (e) {
          showToast(`保存失败: ${formatError(e)}`, 'error')
        }
        saveTimer = null
      }, 300)
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

  async function minimize() {
    try {
      await invoke('ipc_dock_minimize_to_tab')
    } catch (e) {
      showToast(`最小化失败: ${formatError(e)}`, 'error')
    }
  }

  // --- Ctrl+click drag ---

  let ctrlHeld = $state(false)
  let dragOverlay = $state<{ active: boolean; count: number }>({ active: false, count: 0 })

  let pasteConsumed = false

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Control') ctrlHeld = true
    if (e.ctrlKey && e.code === 'KeyV') {
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
        showToast('已收纳图片')
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
        showToast(`已收纳 ${files.length} 个文件`)
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
        showToast('已收纳文本')
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
        showToast('已收纳文本')
      }
    } catch {
      showToast('粘贴失败', 'error')
    }
  }

  async function handleNativeFileDrop(paths: string[]) {
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
      const fileNames = paths.map((p) => p.split(/[\\/]/).pop()).filter(Boolean)
      if (paths.length === 1 && fileNames[0]) {
        showToast(`已收纳文件：${fileNames[0]}`)
      } else {
        showToast(`已收纳 ${paths.length} 个文件`)
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

  function showToast(text: string, kind: 'success' | 'error' = 'success', undo?: () => void, actionLabel?: string) {
    if (toastTimer) clearTimeout(toastTimer)
    toast = { text, kind, undo, actionLabel }
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
  <TopBar {currentView} onNavigate={navigate} onToggleSettings={toggleSettings} onMinimize={minimize} />

  {#if currentView === 'home'}
  <HomeView
    entries={homeEntries}
    onToggleCollapse={toggleCollapse}
    onDeleteFromView={(id) => deleteFromView('home', id)}
    onToggleNote={toggleNote}
    onCreateText={createHomeText}
    onImportEntry={importHomeEntry}
    onUpdateText={updateText}
    onRename={renameEntry}
    onCopy={copyContent}
    onCopyPath={copyPath}
  />
{:else if currentView === 'categories'}
  <CategoriesView
    entries={allEntries}
    onToggleCollapse={toggleCollapse}
    onDeleteFromView={deleteFromAnyView}
    onToggleNote={toggleNote}
    onUpdateText={updateText}
    onRename={renameEntry}
    onCopy={copyContent}
    onCopyPath={copyPath}
  />
{:else if currentView === 'note'}
  <NoteView
    entries={noteEntries}
    onToggleCollapse={toggleCollapse}
    onDeleteFromView={(id) => deleteFromView('note', id)}
    onToggleNote={toggleNote}
    onCreateText={createNoteText}
    onImportEntry={importNoteEntry}
    onUpdateText={updateText}
    onRename={renameEntry}
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
</div>

{#if toast}
  <div class="toast" class:toast-error={toast.kind === 'error'}>
    <span>{toast.text}</span>
    {#if toast.undo}
      <button class="toast-undo" onclick={toast.undo}>{toast.actionLabel || '撤销'}</button>
    {/if}
  </div>
{/if}

{#if dragOverlay.active}
  <div class="drag-overlay">
    <div class="drag-overlay-content">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
        <polyline points="7 10 12 15 17 10" />
        <line x1="12" y1="15" x2="12" y2="3" />
      </svg>
      <span>{dragOverlay.count > 1 ? `释放以收纳 ${dragOverlay.count} 个文件` : '释放以收纳文件'}</span>
    </div>
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
    background: var(--surface-2);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    color: var(--color-primary);
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
    color: var(--color-primary);
    font-weight: 600;
    font-size: 0.7rem;
    cursor: pointer;
    padding: 0;
    font-family: inherit;
    text-decoration: underline;
  }

  .toast-undo:hover {
    opacity: 0.85;
  }

  .toast-error {
    border-color: color-mix(in srgb, var(--color-danger) 30%, transparent);
    color: var(--color-danger);
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

  .drag-overlay {
    position: absolute;
    inset: 0;
    background: color-mix(in srgb, var(--surface-0) 85%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 200;
    border: 2px dashed color-mix(in srgb, var(--color-primary) 50%, transparent);
    border-radius: var(--radius-lg, 0.5rem);
  }

  .drag-overlay-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
    color: var(--color-primary);
    font-size: var(--font-sm, 0.75rem);
    font-weight: 500;
  }
</style>
