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
  entrySurfaceOpacity: number
  dockBgOpacity: number
  dockBgColor: string
  dockMinimized: boolean
  dockPositionX: number
  dockPositionY: number
  dockWidth: number
  dockHeight: number
  dockEdgeAnchor: string
  textSizePx: number
  textColor: string
  fontFamilyZh: string
  fontFamilyEn: string
  launchOnStartup: boolean
  updateProxy: string
}
