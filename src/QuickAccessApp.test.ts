import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import type { DockPreferences } from '$lib/types/dock'
import { loadLocale } from '$lib/i18n'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  hide: vi.fn(),
  isAlwaysOnTop: vi.fn(),
  setAlwaysOnTop: vi.fn(),
  startDragging: vi.fn(),
  getLlmConfig: vi.fn(),
  getAiSettings: vi.fn(),
  searchLocal: vi.fn(),
  planSearch: vi.fn(),
  cancelSearch: vi.fn(),
  getEntry: vi.fn(),
  copyText: vi.fn(),
  contentRevision: vi.fn(),
  contentSearchLocal: vi.fn(),
  contentDetail: vi.fn(),
  contentPlanSearch: vi.fn(),
  contentCancelPlan: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    hide: mocks.hide,
    isAlwaysOnTop: mocks.isAlwaysOnTop,
    setAlwaysOnTop: mocks.setAlwaysOnTop,
    startDragging: mocks.startDragging,
  }),
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
vi.mock('$lib/api/content', () => ({
  contentApi: {
    revision: mocks.contentRevision,
    searchLocal: mocks.contentSearchLocal,
    detail: mocks.contentDetail,
    planSearch: mocks.contentPlanSearch,
    cancelPlan: mocks.contentCancelPlan,
  },
  onContentChanged: async (handler: (event: unknown) => void) => {
    mocks.listeners.set('content-changed', ({ payload }) => handler(payload))
    return vi.fn()
  },
}))

import { PREFERENCES_PREVIEW_EVENT } from '$lib/state/preferences-sync'
import { computeThemeTokens } from '$lib/themes/engine'
import QuickAccessApp from './QuickAccessApp.svelte'

function preferences(language = 'zh-CN'): DockPreferences {
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
    language,
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
  loadLocale('zh-CN')
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    value: vi.fn(() => ({
      matches: true,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  })
  mocks.listeners.clear()
  mocks.listen.mockImplementation(
    async (event: string, handler: (event: { payload: unknown }) => void) => {
      mocks.listeners.set(event, handler)
      return vi.fn()
    },
  )
  mocks.hide.mockResolvedValue(undefined)
  mocks.isAlwaysOnTop.mockResolvedValue(true)
  mocks.setAlwaysOnTop.mockResolvedValue(undefined)
  mocks.startDragging.mockResolvedValue(undefined)
  mocks.getLlmConfig.mockResolvedValue(null)
  mocks.getAiSettings.mockResolvedValue({
    autoEnrich: true,
    autoHybridSearch: false,
    sensitiveClipboardClearSeconds: null,
  })
  mocks.searchLocal.mockResolvedValue([])
  mocks.planSearch.mockResolvedValue(null)
  mocks.cancelSearch.mockResolvedValue(undefined)
  mocks.getEntry.mockResolvedValue(null)
  mocks.copyText.mockResolvedValue(undefined)
  mocks.contentRevision.mockResolvedValue({ revision: 0 })
  mocks.contentSearchLocal.mockResolvedValue([])
  mocks.contentDetail.mockResolvedValue(null)
  mocks.contentPlanSearch.mockResolvedValue(null)
  mocks.contentCancelPlan.mockResolvedValue(undefined)
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
  it('repairs an active search when a newer revision is observed on show', async () => {
    vi.useFakeTimers()
    render(QuickAccessApp)
    await vi.advanceTimersByTimeAsync(0)
    await fireEvent.click(screen.getByRole('tab', { name: '搜索' }))
    await fireEvent.input(screen.getByRole('searchbox'), { target: { value: '数据库' } })
    await vi.advanceTimersByTimeAsync(300)
    expect(mocks.contentSearchLocal).toHaveBeenCalledTimes(1)

    mocks.contentRevision.mockResolvedValueOnce({ revision: 8 })
    mocks.listeners.get('quick-access-focus-input')?.({ payload: undefined })
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(300)

    expect(mocks.contentRevision).toHaveBeenCalled()
    expect(mocks.contentSearchLocal).toHaveBeenCalledTimes(2)
    vi.useRealTimers()
  })

  it('keeps record draft and search query independent across show events', async () => {
    render(QuickAccessApp)
    const record = await screen.findByPlaceholderText('粘贴或输入要保存的内容')
    await fireEvent.input(record, { target: { value: 'draft text' } })
    await fireEvent.click(screen.getByRole('tab', { name: '搜索' }))
    await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'query text' } })
    mocks.listeners.get('quick-access-focus-input')?.({ payload: undefined })
    await fireEvent.click(screen.getByRole('tab', { name: '记录' }))
    expect(screen.getByPlaceholderText('粘贴或输入要保存的内容')).toHaveValue('draft text')
    await fireEvent.click(screen.getByRole('tab', { name: '搜索' }))
    expect(screen.getByRole('searchbox')).toHaveValue('query text')
  })

  it('reads and toggles the actual Quick Access pin state', async () => {
    render(QuickAccessApp)
    const pin = await screen.findByRole('button', { name: '取消置顶' })

    await waitFor(() => expect(mocks.isAlwaysOnTop).toHaveBeenCalledTimes(1))
    await fireEvent.click(pin)

    await waitFor(() => expect(mocks.setAlwaysOnTop).toHaveBeenCalledWith(false))
    expect(screen.getByRole('button', { name: '置顶' })).toBeInTheDocument()
  })

  it('hides and drags only through explicit work-panel actions', async () => {
    render(QuickAccessApp)

    await fireEvent.mouseDown(await screen.findByTestId('quick-access-drag-region'))
    expect(mocks.startDragging).toHaveBeenCalledTimes(1)

    await fireEvent.click(screen.getByRole('button', { name: '关闭' }))
    expect(mocks.hide).toHaveBeenCalledTimes(1)
  })

  it('renders both primary modes as icon-labelled segments', async () => {
    render(QuickAccessApp)
    const record = await screen.findByRole('tab', { name: '记录' })
    const search = screen.getByRole('tab', { name: '搜索' })

    expect(record).toHaveAttribute('data-primary-mode', 'record')
    expect(search).toHaveAttribute('data-primary-mode', 'search')
    expect(record.querySelector('svg[aria-hidden="true"]')).toBeInTheDocument()
    expect(search.querySelector('svg[aria-hidden="true"]')).toBeInTheDocument()
  })

  it('applies live preference changes from the main window', async () => {
    render(QuickAccessApp)
    await screen.findByRole('tab', { name: '记录' })

    const next = preferences()
    next.themePresetId = 'light-matte'
    next.fontFamilyZh = 'SimSun'
    await waitFor(() => {
      expect(mocks.listeners.get(PREFERENCES_PREVIEW_EVENT)).toBeDefined()
    })
    const listener = mocks.listeners.get(PREFERENCES_PREVIEW_EVENT)
    listener?.({ payload: next })

    const expected = computeThemeTokens(next, true)
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue('--surface-0')).toBe(
        expected['--surface-0'],
      )
      expect(document.documentElement.style.getPropertyValue('--font-family-zh')).toBe('SimSun')
    })
  })

  it('renders the persisted English locale after preferences load', async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'ipc_preferences_get') return preferences('en')
      return undefined
    })

    render(QuickAccessApp)

    expect(await screen.findByRole('button', { name: 'Configure now' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Record' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Search' })).toBeInTheDocument()
  })

  it('opens the main Settings view through the dedicated command', async () => {
    render(QuickAccessApp)

    await fireEvent.click(await screen.findByRole('button', { name: '立即配置' }))

    await waitFor(() => {
      expect(mocks.invoke).toHaveBeenCalledWith('ipc_open_main_settings')
    })
    expect(mocks.invoke).not.toHaveBeenCalledWith('ipc_open_quick_access')
  })

  it('preserves record and search input across mode switches', async () => {
    render(QuickAccessApp)

    const recordInput = await screen.findByRole('textbox', { name: '记录' })
    await fireEvent.input(recordInput, { target: { value: 'UXREVIEWDRAFT' } })

    await fireEvent.click(screen.getByRole('tab', { name: '搜索' }))
    const searchInput = screen.getByRole('searchbox', { name: '搜索' })
    await fireEvent.input(searchInput, { target: { value: 'database' } })

    await fireEvent.click(screen.getByRole('tab', { name: '记录' }))
    expect(screen.getByRole('textbox', { name: '记录' })).toHaveValue('UXREVIEWDRAFT')

    await fireEvent.click(screen.getByRole('tab', { name: '搜索' }))
    expect(screen.getByRole('searchbox', { name: '搜索' })).toHaveValue('database')
  })

  it('focuses the active input after mouse and keyboard mode switches', async () => {
    render(QuickAccessApp)

    await fireEvent.click(screen.getByRole('tab', { name: '搜索' }))
    const searchInput = screen.getByRole('searchbox', { name: '搜索' })
    await waitFor(() => expect(searchInput).toHaveFocus())

    await fireEvent.keyDown(window, { key: 'Tab', ctrlKey: true })
    const recordInput = screen.getByRole('textbox', { name: '记录' })
    await waitFor(() => expect(recordInput).toHaveFocus())
  })

  it('connects each tab to a persistent tabpanel', async () => {
    render(QuickAccessApp)

    const recordTab = screen.getByRole('tab', { name: '记录' })
    const searchTab = screen.getByRole('tab', { name: '搜索' })
    expect(recordTab).toHaveAttribute('aria-controls', 'qa-record-panel')
    expect(searchTab).toHaveAttribute('aria-controls', 'qa-search-panel')

    expect(screen.getByRole('tabpanel', { name: '记录' })).not.toHaveAttribute('hidden')
    expect(document.getElementById('qa-search-panel')).toHaveAttribute('hidden')
  })
})
