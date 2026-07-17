import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import type { DockPreferences } from '$lib/types/dock'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  hide: vi.fn(),
  getLlmConfig: vi.fn(),
  getAiSettings: vi.fn(),
  searchLocal: vi.fn(),
  planSearch: vi.fn(),
  cancelSearch: vi.fn(),
  getEntry: vi.fn(),
  copyText: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ hide: mocks.hide }),
}))
vi.mock('$lib/api/vault', () => ({
  vaultApi: {
    getLlmConfig: mocks.getLlmConfig,
    getAiSettings: mocks.getAiSettings,
    searchLocal: mocks.searchLocal,
    planSearch: mocks.planSearch,
    cancelSearch: mocks.cancelSearch,
    getEntry: mocks.getEntry,
    copyText: mocks.copyText,
  },
}))

import QuickAccessApp from './QuickAccessApp.svelte'

function preferences(): DockPreferences {
  return {
    themeMode: 'preset',
    themePresetId: 'dark-glass',
    customBasePresetId: '',
    themeOverrides: {},
    uiTextSizePx: 12,
    contentTextSizePx: 14,
    spacingPreset: 'normal',
    radiusPreset: 'normal',
    dockPositionX: 40,
    dockPositionY: 40,
    dockWidth: 360,
    dockHeight: 640,
    dockEdgeAnchor: 'right',
    dockMinimized: false,
    fontFamilyZh: 'Microsoft YaHei',
    fontFamilyEn: 'Segoe UI',
    launchOnStartup: false,
    updateProxy: '',
    language: 'zh-CN',
    shortcutModifiers: 'Alt+Shift',
    shortcutKey: 'V',
    shortcutRegistered: true,
    quickAccessShortcutModifiers: 'Alt+Shift',
    quickAccessShortcutKey: 'Space',
    quickAccessShortcutRegistered: true,
    autoCleanupDays: 0,
  }
}

beforeEach(() => {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    value: vi.fn(() => ({
      matches: true,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  })
  mocks.listen.mockResolvedValue(vi.fn())
  mocks.hide.mockResolvedValue(undefined)
  mocks.getLlmConfig.mockResolvedValue(null)
  mocks.getAiSettings.mockResolvedValue({
    autoEnrich: true,
    autoHybridSearch: false,
    sensitiveClipboardClearSeconds: null,
  })
  mocks.invoke.mockImplementation(async (command: string) => {
    if (command === 'ipc_preferences_get') return preferences()
    return undefined
  })
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('QuickAccessApp', () => {
  it('opens the main Settings view through the dedicated command', async () => {
    render(QuickAccessApp)

    await fireEvent.click(await screen.findByRole('button', { name: '立即配置' }))

    await waitFor(() => {
      expect(mocks.invoke).toHaveBeenCalledWith('ipc_open_main_settings')
    })
    expect(mocks.invoke).not.toHaveBeenCalledWith('ipc_open_quick_access')
  })
})
