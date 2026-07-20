// Browser preview mock for Tauri IPC — lets the Svelte frontend run in a
// plain browser (vite dev) without the Rust backend. ONLY used by preview.html.
import type { ContentDetail, ContentSummary } from '$lib/types/content'
import type { DockPreferences } from '$lib/types/dock'

const now = Date.now()
const iso = (offsetMin: number) => new Date(now - offsetMin * 60_000).toISOString()

const caps = {
  copyText: true,
  copyImage: false,
  copyFile: false,
  copyPath: false,
  openUrl: false,
  revealSensitive: false,
  edit: true,
  save: true,
  unsave: false,
  delete: true,
  reorder: true,
}

const longText = `Claude Code 报错排查记录

问题：在 monorepo 里运行 pnpm build 时报错 "ERR_MODULE_NOT_FOUND"。

排查步骤：
1. 检查 node_modules 是否完整安装 → pnpm install 后问题依旧
2. 检查 package.json 的 exports 字段 → 发现缺少 "./utils" 导出
3. 修复后重新构建成功

结论：子包新增目录时记得同步 exports 配置。`

const sampleItems: ContentSummary[] = [
  {
    id: 't1',
    kind: 'text',
    retention: 'temporary',
    title: 'Claude Code 报错排查记录',
    preview: '问题：在 monorepo 里运行 pnpm build 时报错 ERR_MODULE_NOT_FOUND…',
    createdAt: iso(12),
    updatedAt: iso(12),
    cleanupAt: iso(-60 * 24 * 3),
    capabilities: { ...caps },
  },
  {
    id: 'i1',
    kind: 'image',
    retention: 'temporary',
    title: '粘贴图片 07/20 14:32',
    preview: 'screenshot-2026-07-20.png · 4.6 MB',
    createdAt: iso(45),
    updatedAt: iso(45),
    cleanupAt: iso(-60 * 24 * 3),
    capabilities: { ...caps, copyText: false, copyImage: true, copyPath: true, edit: false },
  },
  {
    id: 'f1',
    kind: 'file',
    retention: 'saved',
    title: 'README.md',
    preview: 'README.md · 3.6 KB',
    createdAt: iso(60 * 5),
    updatedAt: iso(60 * 5),
    cleanupAt: null,
    capabilities: { ...caps, copyText: false, copyFile: true, copyPath: true, edit: false, save: false, unsave: true },
  },
  {
    id: 't2',
    kind: 'text',
    retention: 'temporary',
    title: 'https://github.com/SiChuchen/Scratchpad',
    preview: 'https://github.com/SiChuchen/Scratchpad.git',
    createdAt: iso(60 * 26),
    updatedAt: iso(60 * 26),
    cleanupAt: iso(-60 * 24 * 2),
    capabilities: { ...caps },
  },
  {
    id: 'n1',
    kind: 'note',
    retention: 'saved',
    title: '常用 Git 命令速查',
    preview: 'git rebase -i HEAD~3 交互式变基…',
    createdAt: iso(60 * 50),
    updatedAt: iso(60 * 50),
    cleanupAt: null,
    capabilities: { ...caps, save: false, unsave: true },
  },
  {
    id: 'b1',
    kind: 'bookmark',
    retention: 'saved',
    title: 'Tauri v2 官方文档',
    preview: 'https://v2.tauri.app/',
    createdAt: iso(60 * 80),
    updatedAt: iso(60 * 80),
    cleanupAt: null,
    capabilities: { ...caps, openUrl: true, save: false, unsave: true },
  },
  {
    id: 'c1',
    kind: 'credential',
    retention: 'saved',
    title: '生产数据库',
    preview: 'prod-db.example.com · admin',
    createdAt: iso(60 * 120),
    updatedAt: iso(60 * 120),
    cleanupAt: null,
    capabilities: { ...caps, revealSensitive: true, save: false, unsave: true },
  },
]

const details: Record<string, ContentDetail> = {
  t1: {
    kind: 'text',
    summary: sampleItems[0] as ContentSummary<'text'>,
    title: 'Claude Code 报错排查记录',
    body: longText,
  },
  i1: {
    kind: 'image',
    summary: sampleItems[1] as ContentSummary<'image'>,
    fileName: 'screenshot-2026-07-20.png',
    assetPath: '/mock/screenshot-2026-07-20.png',
    mimeType: 'image/png',
    width: 2560,
    height: 1440,
    available: true,
  },
  f1: {
    kind: 'file',
    summary: sampleItems[2] as ContentSummary<'file'>,
    fileName: 'README.md',
    assetPath: '/mock/README.md',
    mimeType: 'text/markdown',
    sizeBytes: 3684,
    available: true,
  },
  t2: {
    kind: 'text',
    summary: sampleItems[3] as ContentSummary<'text'>,
    title: 'https://github.com/SiChuchen/Scratchpad',
    body: 'https://github.com/SiChuchen/Scratchpad.git',
  },
}

const preferences: DockPreferences = {
  themeMode: 'preset',
  themePresetId: 'dark-glass',
  customBasePresetId: '',
  themeOverrides: {},
  uiTextSizePx: 12,
  contentTextSizePx: 14,
  spacingPreset: 'normal',
  radiusPreset: 'normal',
  dockPositionX: 100,
  dockPositionY: 100,
  dockWidth: 380,
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
  quickAccessShortcutRegistered: false,
  autoCleanupDays: 7,
}

// ---- event registry ------------------------------------------------------
type Handler = (event: { event: string; payload: unknown }) => void
const listeners = new Map<number, { event: string; handler: Handler }>()
let nextCallbackId = 1

export function emitMockEvent(event: string, payload: unknown) {
  for (const l of listeners.values()) {
    if (l.event === event) l.handler({ event, payload })
  }
}

// ---- invoke dispatch -----------------------------------------------------
async function mockInvoke(cmd: string, args: any): Promise<any> {
  switch (cmd) {
    case 'ipc_preferences_get': {
      const p = structuredClone(preferences)
      const themeOverride = localStorage.getItem('preview-theme')
      if (themeOverride) {
        p.themeMode = 'preset'
        p.themePresetId = themeOverride
      }
      const langOverride = localStorage.getItem('preview-lang')
      if (langOverride) p.language = langOverride
      return p
    }
    case 'ipc_preferences_set':
      Object.assign(preferences, args?.prefs ?? {})
      return
    case 'ipc_preferences_list_fonts':
      return ['Microsoft YaHei', 'Segoe UI', 'SimSun', 'Consolas', 'Cascadia Code']
    case 'ipc_content_list': {
      const scope = args?.scope ?? 'temporary'
      const kind = args?.kind ?? null
      return sampleItems.filter(
        (i) =>
          (scope === 'all' ||
            (scope === 'temporary' && i.retention === 'temporary') ||
            (scope === 'saved' && i.retention === 'saved')) &&
          (!kind || i.kind === kind),
      )
    }
    case 'ipc_content_detail':
      return structuredClone(details[args?.id as string] ?? details.t1)
    case 'ipc_content_revision':
      return { revision: 1 }
    case 'ipc_content_search_local': {
      const q = String(args?.query ?? '').toLowerCase()
      return sampleItems
        .filter(
          (i) =>
            !q ||
            i.title.toLowerCase().includes(q) ||
            (i.preview ?? '').toLowerCase().includes(q),
        )
        .map((summary) => ({ summary, score: 1, sources: ['local'] }))
    }
    case 'ipc_content_plan_search':
      return { kind: 'empty' }
    case 'ipc_content_cancel_search':
      return
    case 'ipc_content_save':
    case 'ipc_content_unsave': {
      const it = sampleItems.find((i) => i.id === args?.id)
      if (it) it.retention = cmd === 'ipc_content_save' ? 'saved' : 'temporary'
      return
    }
    case 'ipc_content_delete':
      return { token: 'mock-undo-token' }
    case 'ipc_content_restore':
    case 'ipc_content_rename':
    case 'ipc_content_update_text':
    case 'ipc_content_reorder':
      return
    case 'ipc_toggle_always_on_top':
      return { always_on_top: true }
    case 'ipc_dock_minimize_to_tab':
    case 'ipc_open_quick_access':
    case 'ipc_open_main_content':
    case 'ipc_open_main_settings':
      return
    case 'ipc_clipboard_copy_file':
    case 'ipc_clipboard_copy_image':
      return
    case 'ipc_clipboard_read_file_paths':
      return []
    case 'ipc_data_dir_info':
      return { path: 'C:\\Apps\\SomaScratchpad\\data', mode: 'portable' }
    case 'ipc_data_dir_set':
      return { path: args?.path ?? '', mode: 'custom' }
    case 'ipc_shortcut_status':
      return { modifiers: 'Alt+Shift', key: 'V', registered: true }
    case 'ipc_shortcut_update':
      return { modifiers: args?.modifiers, key: args?.key, registered: true }
    case 'ipc_entries_create_text':
      return null
    case 'plugin:event|listen': {
      const id: number = args?.handler ?? 0
      const event: string = args?.event ?? ''
      const entry = listeners.get(id)
      if (entry) entry.event = event
      return id
    }
    case 'plugin:event|unlisten':
    case 'plugin:event|emit':
    case 'plugin:event|emit_to':
      return
    case 'plugin:window|is_always_on_top':
      return true
    case 'plugin:window|set_always_on_top':
    case 'plugin:window|start_dragging':
    case 'plugin:window|hide':
    case 'plugin:window|close':
    case 'plugin:window|set_position':
      return
    case 'plugin:window|outer_position':
      return { x: 100, y: 100 }
    case 'plugin:autostart|is_enabled':
      return false
    case 'plugin:updater|check':
      return null
    default:
      if (cmd.startsWith('plugin:')) return null
      console.warn('[mock] unhandled invoke:', cmd, args)
      return null
  }
}

// ---- install -------------------------------------------------------------
export function installTauriMock(label = 'main') {
  ;(window as any).__TAURI_INTERNALS__ = {
    invoke: (cmd: string, args?: any) => Promise.resolve(mockInvoke(cmd, args)),
    transformCallback: (callback: Handler, _once = false) => {
      const id = nextCallbackId++
      listeners.set(id, { event: '', handler: callback })
      return id
    },
    unregisterCallback: (id: number) => {
      listeners.delete(id)
    },
    convertFileSrc: (filePath: string) =>
      `data:image/svg+xml,${encodeURIComponent(
        `<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180"><rect width="100%" height="100%" fill="#3b82f6" opacity="0.45"/><text x="50%" y="52%" fill="#ffffff" font-size="14" text-anchor="middle" font-family="sans-serif">${filePath.split('/').pop()}</text></svg>`,
      )}`,
    metadata: {
      currentWindow: { label },
      currentWebview: { label, windowLabel: label },
    },
  }
  ;(window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: () => {},
  }
}
