export type ThemeMode = 'system' | 'preset' | 'custom'
export type SpacingPreset = 'compact' | 'normal' | 'spacious'
export type RadiusPreset = 'sharp' | 'normal' | 'round'

export interface ThemePreset {
  id: string
  name: string
  tokens: Record<string, string>
}
