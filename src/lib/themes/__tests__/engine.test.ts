import { describe, it, expect } from 'vitest'
import { computeThemeTokens } from '../engine'
import type { DockPreferences } from '$lib/types/dock'
import { THEME_PRESETS } from '../presets'
import { SPACING_TOKENS } from '../layout'

function makePrefs(overrides: Partial<DockPreferences> = {}): DockPreferences {
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
    ...overrides,
  }
}

describe('computeThemeTokens', () => {
  it('returns dark-glass tokens when theme_mode is preset and id is dark-glass', () => {
    const tokens = computeThemeTokens(makePrefs({ themeMode: 'preset', themePresetId: 'dark-glass' }), true)
    expect(tokens['--color-primary']).toBe('rgba(125, 211, 252, 0.9)')
    expect(tokens['--surface-0']).toBe('rgba(42, 53, 72, 0.85)')
  })

  it('returns light-frosted tokens when theme_mode is preset and id is light-frosted', () => {
    const tokens = computeThemeTokens(makePrefs({ themeMode: 'preset', themePresetId: 'light-frosted' }), true)
    expect(tokens['--color-primary']).toBe('rgba(161, 98, 7, 0.8)')
  })

  it('system mode + system dark → dark-glass preset', () => {
    const tokens = computeThemeTokens(makePrefs({ themeMode: 'system' }), true)
    expect(tokens['--color-primary']).toBe(THEME_PRESETS['dark-glass'].tokens['--color-primary'])
  })

  it('system mode + system light → light-frosted preset', () => {
    const tokens = computeThemeTokens(makePrefs({ themeMode: 'system' }), false)
    expect(tokens['--color-primary']).toBe(THEME_PRESETS['light-frosted'].tokens['--color-primary'])
  })

  it('custom mode merges overrides on top of base preset', () => {
    const tokens = computeThemeTokens(makePrefs({
      themeMode: 'custom',
      customBasePresetId: 'dark-glass',
      themeOverrides: { '--color-primary': '#ff0000' },
    }), true)
    expect(tokens['--color-primary']).toBe('#ff0000')
    // Other tokens still from dark-glass base
    expect(tokens['--surface-0']).toBe('rgba(42, 53, 72, 0.85)')
  })

  it('includes spacing tokens from spacingPreset', () => {
    const tokens = computeThemeTokens(makePrefs({ spacingPreset: 'compact' }), true)
    expect(tokens['--space-sm']).toBe('0.15rem')
  })

  it('includes radius tokens from radiusPreset', () => {
    const tokens = computeThemeTokens(makePrefs({ radiusPreset: 'round' }), true)
    expect(tokens['--radius-md']).toBe('0.5rem')
  })

  it('includes derived font tokens from ui/content text size', () => {
    const tokens = computeThemeTokens(makePrefs({ uiTextSizePx: 12, contentTextSizePx: 14 }), true)
    expect(tokens['--font-md']).toBe('12px')
    expect(tokens['--font-body']).toBe('11.9px')
  })

  it('overrides take precedence over preset and layout tokens', () => {
    const tokens = computeThemeTokens(makePrefs({
      themeMode: 'custom',
      customBasePresetId: 'dark-glass',
      spacingPreset: 'normal',
      themeOverrides: { '--space-sm': '0.5rem', '--color-primary': '#custom' },
    }), true)
    expect(tokens['--space-sm']).toBe('0.5rem')
    expect(tokens['--color-primary']).toBe('#custom')
  })

  it('returns 30 tokens total', () => {
    const tokens = computeThemeTokens(makePrefs(), true)
    const keys = Object.keys(tokens)
    // 17 color + 1 shadow + 3 spacing + 3 radius + 6 font = 30
    expect(keys.length).toBe(30)
  })
})
