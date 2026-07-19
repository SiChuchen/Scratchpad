import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { DockPreferences } from '$lib/types/dock'

export const PREFERENCES_PREVIEW_EVENT = 'dock-preferences-preview'

export function broadcastPreferences(prefs: DockPreferences): Promise<void> {
  return emit(PREFERENCES_PREVIEW_EVENT, prefs)
}

export function listenForPreferenceChanges(
  onChange: (prefs: DockPreferences) => void,
): Promise<UnlistenFn> {
  return listen<DockPreferences>(PREFERENCES_PREVIEW_EVENT, (event) => {
    onChange(event.payload)
  })
}
