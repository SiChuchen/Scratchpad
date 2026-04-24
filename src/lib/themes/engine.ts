import type { DockPreferences } from '$lib/types/dock'
import { THEME_PRESETS, DEFAULT_DARK_PRESET, DEFAULT_LIGHT_PRESET } from './presets'
import { SPACING_TOKENS, RADIUS_TOKENS } from './layout'

export function computeThemeTokens(
  prefs: DockPreferences,
  systemDark: boolean,
): Record<string, string> {
  // 1. Determine base preset
  let presetId: string
  switch (prefs.themeMode) {
    case 'system':
      presetId = systemDark ? DEFAULT_DARK_PRESET : DEFAULT_LIGHT_PRESET
      break
    case 'preset':
      presetId = prefs.themePresetId
      break
    case 'custom':
      presetId = prefs.customBasePresetId
      break
    default:
      presetId = DEFAULT_DARK_PRESET
  }

  const preset = THEME_PRESETS[presetId] ?? THEME_PRESETS[DEFAULT_DARK_PRESET]

  // 2. Start with preset tokens
  const tokens: Record<string, string> = { ...preset.tokens }

  // 3. Merge spacing tokens
  const spacing = SPACING_TOKENS[prefs.spacingPreset as keyof typeof SPACING_TOKENS]
    ?? SPACING_TOKENS.normal
  Object.assign(tokens, spacing)

  // 4. Merge radius tokens
  const radius = RADIUS_TOKENS[prefs.radiusPreset as keyof typeof RADIUS_TOKENS]
    ?? RADIUS_TOKENS.normal
  Object.assign(tokens, radius)

  // 5. Compute font tokens
  const ui = prefs.uiTextSizePx
  const content = prefs.contentTextSizePx
  tokens['--font-xs'] = `${Math.round(ui * 0.78 * 10) / 10}px`
  tokens['--font-sm'] = `${Math.round(ui * 0.875 * 10) / 10}px`
  tokens['--font-md'] = `${ui}px`
  tokens['--font-lg'] = `${Math.round(ui * 1.1 * 10) / 10}px`
  tokens['--font-body'] = `${Math.round(content * 0.85 * 10) / 10}px`
  tokens['--font-mono'] = `${Math.round(content * 0.85 * 10) / 10}px`

  // 6. Merge user overrides (highest priority, for custom mode)
  if (prefs.themeMode === 'custom' && prefs.themeOverrides) {
    for (const [key, value] of Object.entries(prefs.themeOverrides)) {
      if (value) tokens[key] = value
    }
  }

  return tokens
}
