import type { ThemePreset } from './types'

const DARK_GLASS: ThemePreset = {
  id: 'dark-glass',
  name: '深色玻璃',
  tokens: {
    '--color-primary': 'rgba(125, 211, 252, 0.9)',
    '--color-primary-light': 'rgba(125, 211, 252, 0.6)',
    '--color-primary-faint': 'rgba(125, 211, 252, 0.12)',
    '--color-accent': 'rgba(251, 191, 36, 0.85)',
    '--color-danger': 'rgba(248, 113, 113, 0.9)',
    '--color-success': 'rgba(74, 222, 128, 0.8)',
    '--color-info': 'rgba(192, 132, 252, 0.85)',
    '--color-file': 'rgba(74, 222, 128, 0.85)',
    '--surface-0': 'rgba(42, 53, 72, 0.95)',
    '--surface-1': 'rgba(255, 255, 255, 0.04)',
    '--surface-2': 'rgba(15, 23, 42, 0.5)',
    '--text-primary': '#e8edf5',
    '--text-muted': 'rgba(148, 163, 184, 0.5)',
    '--text-faint': 'rgba(148, 163, 184, 0.35)',
    '--border-default': 'rgba(148, 163, 184, 0.1)',
    '--border-subtle': 'rgba(148, 163, 184, 0.08)',
    '--border-emphasis': 'rgba(148, 163, 184, 0.25)',
    '--shadow-default': '0 8px 32px rgba(0, 0, 0, 0.45)',
  },
}

const LIGHT_MATTE: ThemePreset = {
  id: 'light-matte',
  name: '磨砂白底',
  tokens: {
    '--color-primary': 'rgba(37, 99, 235, 0.85)',
    '--color-primary-light': 'rgba(37, 99, 235, 0.6)',
    '--color-primary-faint': 'rgba(37, 99, 235, 0.08)',
    '--color-accent': 'rgba(217, 119, 6, 0.85)',
    '--color-danger': 'rgba(220, 38, 38, 0.85)',
    '--color-success': 'rgba(22, 163, 74, 0.8)',
    '--color-info': 'rgba(109, 40, 217, 0.75)',
    '--color-file': 'rgba(22, 163, 74, 0.75)',
    '--surface-0': 'rgba(250, 250, 252, 0.92)',
    '--surface-1': 'rgba(255, 255, 255, 0.85)',
    '--surface-2': 'rgba(0, 0, 0, 0.03)',
    '--text-primary': 'rgba(30, 30, 30, 0.88)',
    '--text-muted': 'rgba(60, 60, 60, 0.45)',
    '--text-faint': 'rgba(60, 60, 60, 0.35)',
    '--border-default': 'rgba(0, 0, 0, 0.07)',
    '--border-subtle': 'rgba(0, 0, 0, 0.05)',
    '--border-emphasis': 'rgba(0, 0, 0, 0.15)',
    '--shadow-default': '0 4px 20px rgba(0, 0, 0, 0.12)',
  },
}

const LIGHT_FROSTED: ThemePreset = {
  id: 'light-frosted',
  name: '半透明奶油',
  tokens: {
    '--color-primary': 'rgba(161, 98, 7, 0.8)',
    '--color-primary-light': 'rgba(161, 98, 7, 0.6)',
    '--color-primary-faint': 'rgba(161, 98, 7, 0.08)',
    '--color-accent': 'rgba(180, 83, 9, 0.85)',
    '--color-danger': 'rgba(185, 28, 28, 0.85)',
    '--color-success': 'rgba(21, 128, 61, 0.8)',
    '--color-info': 'rgba(147, 51, 234, 0.7)',
    '--color-file': 'rgba(21, 128, 61, 0.7)',
    '--surface-0': 'rgba(245, 243, 238, 0.92)',
    '--surface-1': 'rgba(255, 255, 255, 0.55)',
    '--surface-2': 'rgba(0, 0, 0, 0.04)',
    '--text-primary': 'rgba(40, 35, 30, 0.88)',
    '--text-muted': 'rgba(80, 70, 60, 0.45)',
    '--text-faint': 'rgba(80, 70, 60, 0.35)',
    '--border-default': 'rgba(0, 0, 0, 0.06)',
    '--border-subtle': 'rgba(0, 0, 0, 0.05)',
    '--border-emphasis': 'rgba(0, 0, 0, 0.12)',
    '--shadow-default': '0 4px 24px rgba(0, 0, 0, 0.1)',
  },
}

export const THEME_PRESETS: Record<string, ThemePreset> = {
  'dark-glass': DARK_GLASS,
  'light-matte': LIGHT_MATTE,
  'light-frosted': LIGHT_FROSTED,
}

export const DEFAULT_DARK_PRESET = 'dark-glass'
export const DEFAULT_LIGHT_PRESET = 'light-frosted'
