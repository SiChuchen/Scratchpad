import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { DockPreferences } from '$lib/types/dock'

const mocks = vi.hoisted(() => ({ emit: vi.fn(), listen: vi.fn() }))

vi.mock('@tauri-apps/api/event', () => ({
  emit: mocks.emit,
  listen: mocks.listen,
}))

import {
  PREFERENCES_PREVIEW_EVENT,
  broadcastPreferences,
  listenForPreferenceChanges,
} from './preferences-sync'

const prefs = { themePresetId: 'light-matte' } as DockPreferences

beforeEach(() => {
  vi.clearAllMocks()
  mocks.emit.mockResolvedValue(undefined)
  mocks.listen.mockResolvedValue(vi.fn())
})

describe('preferences sync', () => {
  it('broadcasts the complete preference snapshot', async () => {
    await broadcastPreferences(prefs)
    expect(mocks.emit).toHaveBeenCalledWith(PREFERENCES_PREVIEW_EVENT, prefs)
  })

  it('unwraps the Tauri event payload for subscribers', async () => {
    const onChange = vi.fn()
    await listenForPreferenceChanges(onChange)
    mocks.listen.mock.calls[0][1]({ payload: prefs })
    expect(onChange).toHaveBeenCalledWith(prefs)
  })
})
