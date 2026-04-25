export type EntryKind = 'text' | 'image' | 'file'
export type DockView = 'home' | 'categories' | 'note' | 'settings'
export type EntryMembershipView = 'home' | 'note'

export interface DockEntry {
  id: string
  kind: EntryKind
  content: string | null
  filePath: string | null
  fileName: string | null
  mimeType: string | null
  width: number | null
  height: number | null
  sizeBytes: number | null
  collapsed: boolean
  title: string | null
  inHome: boolean
  inNote: boolean
  source: string
  createdAt: string
  updatedAt: string
}

export interface DockPreferences {
  // Theme
  themeMode: 'system' | 'preset' | 'custom'
  themePresetId: string
  customBasePresetId: string
  themeOverrides: Record<string, string>

  // Layout
  uiTextSizePx: number
  contentTextSizePx: number
  spacingPreset: 'compact' | 'normal' | 'spacious'
  radiusPreset: 'sharp' | 'normal' | 'round'

  // Window geometry
  dockPositionX: number
  dockPositionY: number
  dockWidth: number
  dockHeight: number
  dockEdgeAnchor: string
  dockMinimized: boolean

  // Fonts
  fontFamilyZh: string
  fontFamilyEn: string

  // System
  launchOnStartup: boolean
  updateProxy: string

  // Language
  language: string

  // Cleanup
  autoCleanupDays: number
}
