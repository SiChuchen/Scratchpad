import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import type { DockEntry, DockPreferences, EntryKind, EntryMembershipView, ShortcutStatus } from '$lib/types/dock'

export const dockApi = {
  createText(view: EntryMembershipView, content: string, source = 'manual') {
    return invoke<DockEntry>('ipc_entries_create_text', { view, content, source })
  },

  listEntries(view: EntryMembershipView, kind?: EntryKind) {
    return invoke<DockEntry[]>('ipc_entries_list', { view, kind: kind ?? null })
  },

  addToNote(entryId: string) {
    return invoke<void>('ipc_entries_add_to_note', { entryId })
  },

  removeFromView(view: EntryMembershipView, entryId: string) {
    return invoke<void>('ipc_entries_remove_from_view', { view, entryId })
  },

  updateText(id: string, content: string) {
    return invoke<void>('ipc_entries_update_text', { id, content })
  },

  toggleCollapse(id: string, collapsed: boolean) {
    return invoke<void>('ipc_entries_toggle_collapse', { id, collapsed })
  },

  reorderEntries(view: EntryMembershipView, orderedIds: string[]) {
    return invoke<void>('ipc_entries_reorder', { view, orderedIds })
  },

  rename(id: string, title: string | null) {
    return invoke<void>('ipc_entries_rename', { id, title })
  },

  getPreferences() {
    return invoke<DockPreferences>('ipc_preferences_get')
  },

  setPreferences(prefs: DockPreferences) {
    return invoke<void>('ipc_preferences_set', { prefs })
  },

  previewUrl(path: string) {
    return convertFileSrc(path)
  },

  async importImageBlob(blob: Blob, fileName: string, view: EntryMembershipView) {
    const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()))
    return invoke<DockEntry>('ipc_entries_import_image_bytes', {
      bytes,
      fileName,
      mimeType: blob.type || 'image/png',
      width: null,
      height: null,
      view,
    })
  },

  importFile(sourcePath: string, view: EntryMembershipView) {
    return invoke<DockEntry>('ipc_entries_import_file', { sourcePath, view })
  },

  async importFileBlob(blob: Blob, fileName: string, view: EntryMembershipView) {
    const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()))
    return invoke<DockEntry>('ipc_entries_import_file_bytes', {
      bytes,
      fileName,
      mimeType: blob.type || null,
      view,
    })
  },

  listFonts() {
    return invoke<string[]>('ipc_preferences_list_fonts')
  },

  copyFile(path: string) {
    return invoke<void>('ipc_clipboard_copy_file', { path })
  },

  copyImage(path: string) {
    return invoke<void>('ipc_clipboard_copy_image', { path })
  },

  getShortcutStatus() {
    return invoke<ShortcutStatus>('ipc_shortcut_status')
  },

  updateShortcut(modifiers: string, key: string) {
    return invoke<ShortcutStatus>('ipc_shortcut_update', { modifiers, key })
  },

  async toggleAlwaysOnTop(): Promise<boolean> {
    const res = await invoke<{ always_on_top: boolean }>('ipc_toggle_always_on_top')
    return res.always_on_top
  },
}
