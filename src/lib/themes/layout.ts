import type { SpacingPreset, RadiusPreset } from './types'

export const SPACING_TOKENS: Record<SpacingPreset, Record<string, string>> = {
  compact:  { '--space-sm': '0.15rem', '--space-md': '0.25rem', '--space-lg': '0.4rem' },
  normal:   { '--space-sm': '0.2rem',  '--space-md': '0.35rem', '--space-lg': '0.55rem' },
  spacious: { '--space-sm': '0.25rem', '--space-md': '0.45rem', '--space-lg': '0.7rem' },
}

export const RADIUS_TOKENS: Record<RadiusPreset, Record<string, string>> = {
  sharp:  { '--radius-sm': '0.125rem', '--radius-md': '0.2rem',  '--radius-lg': '0.3rem' },
  normal: { '--radius-sm': '0.25rem',   '--radius-md': '0.35rem', '--radius-lg': '0.5rem' },
  round:  { '--radius-sm': '0.375rem',  '--radius-md': '0.5rem',  '--radius-lg': '0.75rem' },
}
