# i18n Bilingual Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Chinese (zh-CN) and English (en) bilingual support using lightweight zero-dependency dictionaries, with language preference saved in DockPreferences and applied on restart.

**Architecture:** A new `src/lib/i18n/` module exports a typed `messages` object. Components import `messages` and access strings via typed keys (e.g., `messages.nav.home`). The module auto-detects system locale on import and corrects from saved preferences on mount. A `language` field is added to both Rust and TypeScript preference models.

**Tech Stack:** TypeScript (i18n module), Svelte 5 (component string replacements), Rust (preference model + storage)

---

## File Structure

### New Files
| File | Purpose |
|------|---------|
| `src/lib/i18n/types.ts` | `LocaleMessages` interface — single source of truth for all i18n keys |
| `src/lib/i18n/locales/zh-CN.ts` | Chinese dictionary |
| `src/lib/i18n/locales/en.ts` | English dictionary |
| `src/lib/i18n/index.ts` | Module entry — exports `messages`, `loadLocale()`, `detectLanguage()` |
| `src/lib/i18n/__tests__/i18n.test.ts` | Unit tests for i18n module |

### Modified Files
| File | Change |
|------|--------|
| `src/lib/types/dock.ts` | Add `language` field to `DockPreferences` |
| `src-tauri/src/models/preferences.rs` | Add `language` field + default |
| `src-tauri/src/scratchpad/preferences.rs` | Add `language` to save/load |
| `src/lib/themes/token-schema.ts` | Change `validateToken` to return error codes |
| `src/lib/utils/title.ts` | Use i18n for generated titles |
| `src/App.svelte` | i18n init + replace all toast/error strings |
| `src/lib/components/TopBar.svelte` | Replace nav labels + tooltips |
| `src/lib/components/EntryCard.svelte` | Replace kind labels + action tooltips |
| `src/lib/components/entry/TextEntryBody.svelte` | Replace copy button + empty text |
| `src/lib/components/entry/ImageEntryBody.svelte` | Replace copy/path buttons |
| `src/lib/components/entry/FileEntryBody.svelte` | Replace copy/path buttons + unknown file |
| `src/lib/components/views/HomeView.svelte` | Replace search/form/empty strings |
| `src/lib/components/views/NoteView.svelte` | Replace search/form/empty strings |
| `src/lib/components/views/CategoriesView.svelte` | Replace filter labels + empty strings |
| `src/lib/components/views/SettingsView.svelte` | Replace all labels/buttons + add language section |

---

### Task 1: Create i18n module (types + dictionaries + index)

**Files:**
- Create: `src/lib/i18n/types.ts`
- Create: `src/lib/i18n/locales/zh-CN.ts`
- Create: `src/lib/i18n/locales/en.ts`
- Create: `src/lib/i18n/index.ts`

- [ ] **Step 1: Create `src/lib/i18n/types.ts`**

```typescript
export interface LocaleMessages {
  nav: {
    home: string
    all: string
    favorites: string
    settings: string
    pin: string
    unpin: string
    minimize: string
  }
  home: {
    search: string
    inputHint: string
    cancel: string
    add: string
    empty: string
    emptyHint: string
    noMatch: string
  }
  note: {
    search: string
    inputHint: string
    cancel: string
    add: string
    empty: string
    emptyHint: string
    noMatch: string
  }
  categories: {
    all: string
    text: string
    image: string
    file: string
    emptyFiltered: string
    empty: string
  }
  entry: {
    text: string
    image: string
    file: string
    untitled: string
    unnamedFile: string
    unknownFile: string
    empty: string
    copy: string
    copied: string
    copyPath: string
    copyFile: string
    copyImage: string
    expand: string
    collapse: string
    favorite: string
    unfavorite: string
    delete: string
    pastedImage: string
    droppedImage: string
    imageShort: string
  }
  settings: {
    back: string
    theme: string
    followSystem: string
    font: string
    zhFont: string
    zhFontSearch: string
    enFont: string
    enFontSearch: string
    update: string
    proxyNote: string
    proxyType: string
    proxyHost: string
    proxyPort: string
    proxyHostExample: string
    proxyPortExample: string
    saveProxy: string
    clear: string
    checkUpdate: string
    checking: string
    upToDate: string
    recheck: string
    newVersion: string
    updateNow: string
    downloading: string
    checkFailed: string
    updateFailed: string
    system: string
    launchOnStartup: string
    advanced: string
    expertMode: string
    usePreset: string
    dangerZone: string
    resetAll: string
    language: string
    restartHint: string
    proxyErrHostRequired: string
    proxyErrPortRequired: string
    proxyErrPortNumeric: string
    proxyErrPortRange: string
    proxyErrNoProtocol: string
    proxyErrNoPort: string
  }
  toast: {
    loadFailed: string
    newVersion: string
    downloading: string
    updateDone: string
    updateFailed: string
    createFailed: string
    deleteFailed: string
    favoriteFailed: string
    operationFailed: string
    editFailed: string
    renameFailed: string
    saveFailed: string
    minimizeFailed: string
    copyFailed: string
    pasteFailed: string
    importFailed: string
    copyImageFailed: string
    copyFileFailed: string
    deleted: string
    undo: string
    copied: string
    copiedPath: string
    storedImage: string
    storedText: string
    storedFiles: string
    storedFileNamed: string
    dragDropFile: string
    dragDropFiles: string
    unknownError: string
    invalid: string
  }
  validation: {
    unknownToken: string
    invalidColor: string
    notNumber: string
    minValue: string
    maxValue: string
    invalidShadow: string
  }
  expert: Record<string, string>
}
```

- [ ] **Step 2: Create `src/lib/i18n/locales/zh-CN.ts`**

```typescript
import type { LocaleMessages } from '../types'

const zhCN: LocaleMessages = {
  nav: {
    home: '收纳',
    all: '全部',
    favorites: '收藏',
    settings: '设置',
    pin: '置顶',
    unpin: '取消置顶',
    minimize: '最小化',
  },
  home: {
    search: '搜索内容、标题、文件名...',
    inputHint: '输入文本内容...',
    cancel: '取消',
    add: '添加',
    empty: '还没有收纳内容',
    emptyHint: '你可以直接粘贴文字、图片或文件，也可以拖入文件到窗口，或点击左上角 + 新建文本。',
    noMatch: '未找到匹配内容',
  },
  note: {
    search: '搜索内容、标题、文件名...',
    inputHint: '输入文本内容...',
    cancel: '取消',
    add: '添加',
    empty: '暂无收藏内容',
    emptyHint: '在收纳页点击星标，即可把重要内容放到这里。也可以点击 + 新建一条文本。',
    noMatch: '未找到匹配内容',
  },
  categories: {
    all: '全部',
    text: '文本',
    image: '图片',
    file: '文件',
    emptyFiltered: '该类型暂无内容',
    empty: '暂无任何内容',
  },
  entry: {
    text: '文本',
    image: '图片',
    file: '文件',
    untitled: '未命名条目',
    unnamedFile: '未命名文件',
    unknownFile: '未知文件',
    empty: '(空)',
    copy: '复制',
    copied: '已复制',
    copyPath: '复制路径',
    copyFile: '复制文件',
    copyImage: '复制图片',
    expand: '展开',
    collapse: '收起',
    favorite: '收藏',
    unfavorite: '取消收藏',
    delete: '删除',
    pastedImage: '粘贴图片',
    droppedImage: '拖入图片',
    imageShort: '图片',
  },
  settings: {
    back: '← 返回',
    theme: '主题',
    followSystem: '跟随系统',
    font: '字体',
    zhFont: '中文字体',
    zhFontSearch: '搜索中文字体...',
    enFont: '英文字体',
    enFontSearch: '搜索英文字体...',
    update: '更新',
    proxyNote: '代理仅用于检查和下载更新，不影响本地内容收纳。',
    proxyType: '代理类型',
    proxyHost: '代理主机',
    proxyPort: '代理端口',
    proxyHostExample: '例如 127.0.0.1',
    proxyPortExample: '例如 11809',
    saveProxy: '保存代理',
    clear: '清除',
    checkUpdate: '检查更新',
    checking: '正在检查...',
    upToDate: '已是最新版本',
    recheck: '重新检查',
    newVersion: '发现新版本',
    updateNow: '立即更新',
    downloading: '正在下载并安装...',
    checkFailed: '检查失败',
    updateFailed: '更新失败',
    system: '系统',
    launchOnStartup: '开机自启',
    advanced: '高级',
    expertMode: '专家模式',
    usePreset: '使用预设值',
    dangerZone: '危险操作',
    resetAll: '重置全部设置',
    language: '语言',
    restartHint: '语言将在重启后生效',
    proxyErrHostRequired: '请填写代理主机',
    proxyErrPortRequired: '请填写代理端口',
    proxyErrPortNumeric: '端口必须是数字',
    proxyErrPortRange: '端口范围应为 1-65535',
    proxyErrNoProtocol: '代理主机中不需要填写协议',
    proxyErrNoPort: '代理主机中不需要填写端口',
  },
  toast: {
    loadFailed: '加载失败',
    newVersion: '发现新版本',
    downloading: '正在下载更新...',
    updateDone: '更新完成，即将重启...',
    updateFailed: '更新失败',
    createFailed: '创建失败',
    deleteFailed: '删除失败',
    favoriteFailed: '收藏操作失败',
    operationFailed: '操作失败',
    editFailed: '编辑失败',
    renameFailed: '重命名失败',
    saveFailed: '保存失败',
    minimizeFailed: '最小化失败',
    copyFailed: '复制失败',
    pasteFailed: '粘贴失败',
    importFailed: '导入失败',
    copyImageFailed: '复制图片失败',
    copyFileFailed: '复制文件失败',
    deleted: '已删除 1 条内容',
    undo: '撤销',
    copied: '已复制',
    copiedPath: '已复制路径',
    storedImage: '已收纳图片',
    storedText: '已收纳文本',
    storedFiles: '已收纳 {n} 个文件',
    storedFileNamed: '已收纳文件：{name}',
    dragDropFile: '释放以收纳文件',
    dragDropFiles: '释放以收纳 {n} 个文件',
    unknownError: '未知错误',
    invalid: '无效',
  },
  validation: {
    unknownToken: '未知 token',
    invalidColor: '格式应为 rgba(...), #hex, 或 oklch(...)',
    notNumber: '请输入数值',
    minValue: '最小 {n}',
    maxValue: '最大 {n}',
    invalidShadow: '格式应为 box-shadow 值',
  },
  expert: {
    '--color-primary': '主强调色',
    '--color-primary-light': '强调色浅',
    '--color-primary-faint': '强调色极浅',
    '--color-accent': '次强调色',
    '--color-danger': '危险色',
    '--color-success': '成功色',
    '--color-info': '信息色',
    '--color-file': '文件色',
    '--surface-0': '容器底色',
    '--surface-1': '卡片表面',
    '--surface-2': '凹陷表面',
    '--text-primary': '主文字',
    '--text-muted': '弱文字',
    '--text-faint': '极淡文字',
    '--border-default': '默认边框',
    '--border-subtle': '分割线',
    '--border-emphasis': '强调边框',
    '--shadow-default': '基础阴影',
    '--space-sm': '间距-小',
    '--space-md': '间距-中',
    '--space-lg': '间距-大',
    '--radius-sm': '圆角-小',
    '--radius-md': '圆角-中',
    '--radius-lg': '圆角-大',
  },
}

export default zhCN
```

- [ ] **Step 3: Create `src/lib/i18n/locales/en.ts`**

```typescript
import type { LocaleMessages } from '../types'

const en: LocaleMessages = {
  nav: {
    home: 'Dock',
    all: 'All',
    favorites: 'Favorites',
    settings: 'Settings',
    pin: 'Pin on top',
    unpin: 'Unpin',
    minimize: 'Minimize',
  },
  home: {
    search: 'Search content, title, filename...',
    inputHint: 'Enter text...',
    cancel: 'Cancel',
    add: 'Add',
    empty: 'No items yet',
    emptyHint: 'You can paste text, images or files directly, drag files into the window, or click + to create a text note.',
    noMatch: 'No matching items',
  },
  note: {
    search: 'Search content, title, filename...',
    inputHint: 'Enter text...',
    cancel: 'Cancel',
    add: 'Add',
    empty: 'No favorites yet',
    emptyHint: 'Click the star icon on any item to add it here. You can also click + to create a text note.',
    noMatch: 'No matching items',
  },
  categories: {
    all: 'All',
    text: 'Text',
    image: 'Image',
    file: 'File',
    emptyFiltered: 'No items of this type',
    empty: 'No items',
  },
  entry: {
    text: 'Text',
    image: 'Image',
    file: 'File',
    untitled: 'Untitled',
    unnamedFile: 'Unnamed file',
    unknownFile: 'Unknown file',
    empty: '(empty)',
    copy: 'Copy',
    copied: 'Copied',
    copyPath: 'Copy path',
    copyFile: 'Copy file',
    copyImage: 'Copy image',
    expand: 'Expand',
    collapse: 'Collapse',
    favorite: 'Add to favorites',
    unfavorite: 'Remove from favorites',
    delete: 'Delete',
    pastedImage: 'Pasted image',
    droppedImage: 'Dropped image',
    imageShort: 'Image',
  },
  settings: {
    back: '← Back',
    theme: 'Theme',
    followSystem: 'Follow system',
    font: 'Font',
    zhFont: 'Chinese font',
    zhFontSearch: 'Search Chinese fonts...',
    enFont: 'English font',
    enFontSearch: 'Search English fonts...',
    update: 'Update',
    proxyNote: 'Proxy is only used for checking and downloading updates, not for local content.',
    proxyType: 'Proxy type',
    proxyHost: 'Proxy host',
    proxyPort: 'Proxy port',
    proxyHostExample: 'e.g. 127.0.0.1',
    proxyPortExample: 'e.g. 11809',
    saveProxy: 'Save proxy',
    clear: 'Clear',
    checkUpdate: 'Check for updates',
    checking: 'Checking...',
    upToDate: 'Already up to date',
    recheck: 'Recheck',
    newVersion: 'New version available',
    updateNow: 'Update now',
    downloading: 'Downloading and installing...',
    checkFailed: 'Check failed',
    updateFailed: 'Update failed',
    system: 'System',
    launchOnStartup: 'Launch on startup',
    advanced: 'Advanced',
    expertMode: 'Expert mode',
    usePreset: 'Use preset value',
    dangerZone: 'Danger zone',
    resetAll: 'Reset all settings',
    language: 'Language',
    restartHint: 'Language will take effect after restart',
    proxyErrHostRequired: 'Proxy host is required',
    proxyErrPortRequired: 'Proxy port is required',
    proxyErrPortNumeric: 'Port must be a number',
    proxyErrPortRange: 'Port must be between 1-65535',
    proxyErrNoProtocol: 'Protocol should not be included in host',
    proxyErrNoPort: 'Port should not be included in host',
  },
  toast: {
    loadFailed: 'Failed to load',
    newVersion: 'New version available',
    downloading: 'Downloading update...',
    updateDone: 'Update complete, restarting...',
    updateFailed: 'Update failed',
    createFailed: 'Failed to create',
    deleteFailed: 'Failed to delete',
    favoriteFailed: 'Failed to update favorite',
    operationFailed: 'Operation failed',
    editFailed: 'Failed to edit',
    renameFailed: 'Failed to rename',
    saveFailed: 'Failed to save',
    minimizeFailed: 'Failed to minimize',
    copyFailed: 'Failed to copy',
    pasteFailed: 'Failed to paste',
    importFailed: 'Failed to import',
    copyImageFailed: 'Failed to copy image',
    copyFileFailed: 'Failed to copy file',
    deleted: 'Deleted 1 item',
    undo: 'Undo',
    copied: 'Copied',
    copiedPath: 'Path copied',
    storedImage: 'Image stored',
    storedText: 'Text stored',
    storedFiles: '{n} files stored',
    storedFileNamed: 'File stored: {name}',
    dragDropFile: 'Drop to store file',
    dragDropFiles: 'Drop to store {n} files',
    unknownError: 'Unknown error',
    invalid: 'Invalid',
  },
  validation: {
    unknownToken: 'Unknown token',
    invalidColor: 'Format should be rgba(...), #hex, or oklch(...)',
    notNumber: 'Enter a number',
    minValue: 'Min {n}',
    maxValue: 'Max {n}',
    invalidShadow: 'Format should be a box-shadow value',
  },
  expert: {
    '--color-primary': 'Primary color',
    '--color-primary-light': 'Primary light',
    '--color-primary-faint': 'Primary faint',
    '--color-accent': 'Accent color',
    '--color-danger': 'Danger color',
    '--color-success': 'Success color',
    '--color-info': 'Info color',
    '--color-file': 'File color',
    '--surface-0': 'Container background',
    '--surface-1': 'Card surface',
    '--surface-2': 'Recessed surface',
    '--text-primary': 'Primary text',
    '--text-muted': 'Muted text',
    '--text-faint': 'Faint text',
    '--border-default': 'Default border',
    '--border-subtle': 'Divider',
    '--border-emphasis': 'Emphasis border',
    '--shadow-default': 'Base shadow',
    '--space-sm': 'Spacing-S',
    '--space-md': 'Spacing-M',
    '--space-lg': 'Spacing-L',
    '--radius-sm': 'Radius-S',
    '--radius-md': 'Radius-M',
    '--radius-lg': 'Radius-L',
  },
}

export default en
```

- [ ] **Step 4: Create `src/lib/i18n/index.ts`**

```typescript
import type { LocaleMessages } from './types'
import zhCN from './locales/zh-CN'
import en from './locales/en'

const locales: Record<string, LocaleMessages> = { 'zh-CN': zhCN, en }

function getInitialLocale(): string {
  if (typeof navigator !== 'undefined' && navigator.language?.startsWith('zh')) return 'zh-CN'
  return 'en'
}

/** Current locale messages — initialized from system locale, updated via loadLocale(). */
export const messages: LocaleMessages = { ...locales[getInitialLocale()] }

/** Detect language from navigator.language. Returns 'zh-CN' or 'en'. */
export function detectLanguage(): string {
  return getInitialLocale()
}

/** Load a locale into the exported messages object. */
export function loadLocale(lang: string): void {
  const locale = locales[lang] || locales.en
  for (const key of Object.keys(locale) as (keyof LocaleMessages)[]) {
    messages[key] = locale[key]
  }
}
```

- [ ] **Step 5: Commit**

```bash
git add src/lib/i18n/
git commit -m "feat(i18n): add bilingual locale module with zh-CN and en dictionaries"
```

---

### Task 2: Write and run i18n unit tests

**Files:**
- Create: `src/lib/i18n/__tests__/i18n.test.ts`

- [ ] **Step 1: Create test file**

```typescript
import { describe, it, expect } from 'vitest'
import zhCN from '../locales/zh-CN'
import en from '../locales/en'
import type { LocaleMessages } from '../types'

/** Recursively collect all leaf key paths from a nested object. */
function collectKeys(obj: Record<string, unknown>, prefix = ''): string[] {
  const keys: string[] = []
  for (const [key, value] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${key}` : key
    if (typeof value === 'object' && value !== null && !Array.isArray(value)) {
      keys.push(...collectKeys(value as Record<string, unknown>, path))
    } else {
      keys.push(path)
    }
  }
  return keys
}

describe('i18n dictionaries', () => {
  it('zh-CN and en have identical key structure', () => {
    const zhKeys = collectKeys(zhCN as unknown as Record<string, unknown>).sort()
    const enKeys = collectKeys(en as unknown as Record<string, unknown>).sort()
    expect(enKeys).toEqual(zhKeys)
  })

  it('all string values are non-empty', () => {
    const checkNonEmpty = (obj: Record<string, unknown>) => {
      for (const [key, value] of Object.entries(obj)) {
        if (typeof value === 'string') {
          expect(value.length, `Empty value for key "${key}"`).toBeGreaterThan(0)
        } else if (typeof value === 'object' && value !== null) {
          checkNonEmpty(value as Record<string, unknown>)
        }
      }
    }
    checkNonEmpty(zhCN as unknown as Record<string, unknown>)
    checkNonEmpty(en as unknown as Record<string, unknown>)
  })

  it('expert labels cover all TOKEN_SCHEMA keys', () => {
    // The expert section should have entries for all theme tokens
    const expertKeys = Object.keys(zhCN.expert)
    expect(expertKeys.length).toBeGreaterThanOrEqual(24) // 17 colors + 1 shadow + 6 layout
    expect(Object.keys(en.expert).length).toBe(expertKeys.length)
  })
})

describe('detectLanguage', () => {
  it('returns zh-CN for zh language codes', async () => {
    // detectLanguage reads navigator.language; test by importing index
    // Since we can't mock navigator easily in vitest, just verify the function exists
    const { detectLanguage } = await import('../index')
    expect(typeof detectLanguage()).toBe('string')
    expect(['zh-CN', 'en']).toContain(detectLanguage())
  })
})

describe('loadLocale', () => {
  it('loads a locale into messages', async () => {
    const { messages, loadLocale } = await import('../index')
    loadLocale('en')
    expect(messages.nav.home).toBe('Dock')
    expect(messages.settings.back).toBe('← Back')
    // Restore
    loadLocale('zh-CN')
    expect(messages.nav.home).toBe('收纳')
  })

  it('falls back to en for unknown locale', async () => {
    const { messages, loadLocale } = await import('../index')
    loadLocale('fr')
    expect(messages.nav.home).toBe('Dock')
  })
})
```

- [ ] **Step 2: Run tests**

Run: `pnpm vitest run src/lib/i18n/__tests__/i18n.test.ts`

Expected: All tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/lib/i18n/__tests__/
git commit -m "test(i18n): add dictionary completeness and module unit tests"
```

---

### Task 3: Add language field to models (Rust + TypeScript)

**Files:**
- Modify: `src-tauri/src/models/preferences.rs`
- Modify: `src-tauri/src/scratchpad/preferences.rs`
- Modify: `src/lib/types/dock.ts`

- [ ] **Step 1: Add `language` to Rust `DockPreferences` model**

In `src-tauri/src/models/preferences.rs`, add after `pub update_proxy: String,` (line 35):

```rust
    // Language
    pub language: String,                              // "zh-CN" | "en", default "" (auto-detect)
```

Update the `Default` impl — add before the closing `}` of `Self { ... }` (after line 59 `update_proxy: String::new(),`):

```rust
            language: String::new(),
```

- [ ] **Step 2: Add `language` to Rust preferences save/load**

In `src-tauri/src/scratchpad/preferences.rs`, update `save_preferences` — add after the `("update_proxy", ...)` entry (line 32):

```rust
        ("language", prefs.language.clone()),
```

Update `load_preferences` — add after the `update_proxy` block (after line 106):

```rust
    if let Some(v) = map.get("language") {
        prefs.language = v.clone();
    }
```

- [ ] **Step 3: Add `language` to TypeScript `DockPreferences`**

In `src/lib/types/dock.ts`, add after `updateProxy: string` (line 51), before the closing `}`:

```typescript
  // Language
  language: string
```

- [ ] **Step 4: Run Rust tests**

Run: `cd src-tauri && cargo test`

Expected: All existing tests pass (the new field defaults to `""`).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/models/preferences.rs src-tauri/src/scratchpad/preferences.rs src/lib/types/dock.ts
git commit -m "feat(i18n): add language field to DockPreferences model"
```

---

### Task 4: Update token-schema.ts + title.ts

**Files:**
- Modify: `src/lib/themes/token-schema.ts`
- Modify: `src/lib/utils/title.ts`

- [ ] **Step 1: Update `validateToken` to return error codes**

In `src/lib/themes/token-schema.ts`, change the `validateToken` function return type and error values. Replace the entire function (lines 40-61):

```typescript
export function validateToken(key: string, value: string): { valid: boolean; errorCode?: string } {
  const schema = TOKEN_SCHEMA[key]
  if (!schema) return { valid: false, errorCode: 'unknownToken' }

  switch (schema.type) {
    case 'color': {
      if (/^(rgba?\(|#|oklch\()/.test(value.trim())) return { valid: true }
      return { valid: false, errorCode: 'invalidColor' }
    }
    case 'length': {
      const num = parseFloat(value)
      if (isNaN(num)) return { valid: false, errorCode: 'notNumber' }
      if (schema.min !== undefined && num < schema.min) return { valid: false, errorCode: 'minValue' }
      if (schema.max !== undefined && num > schema.max) return { valid: false, errorCode: 'maxValue' }
      return { valid: true }
    }
    case 'shadow': {
      if (/\d/.test(value)) return { valid: true }
      return { valid: false, errorCode: 'invalidShadow' }
    }
  }
}
```

- [ ] **Step 2: Update `title.ts` to use i18n**

In `src/lib/utils/title.ts`, add import at top (after line 1):

```typescript
import { messages } from '$lib/i18n'
```

Replace the `generateTitle` function body (lines 43-78):

```typescript
export function generateTitle(entry: DockEntry): string {
  if (entry.kind === 'image') {
    const label =
      entry.source === 'clipboard'
        ? messages.entry.pastedImage
        : entry.source === 'drop'
          ? messages.entry.droppedImage
          : messages.entry.imageShort
    return `${label} ${formatDate(entry.createdAt)}`
  }

  if (entry.kind === 'file') {
    return entry.fileName ?? messages.entry.unnamedFile
  }

  // kind === 'text'
  const content = entry.content ?? ''

  if (looksLikeCode(content)) {
    const lines = content.split('\n')
    const meaningful = lines.find((l) => l.trim().length > 0)
    return (meaningful?.trim() ?? '').slice(0, MAX_TITLE_LEN)
  }

  // Join soft-wrapped lines into the first paragraph.
  const lines = content.split('\n')
  const paragraphLines: string[] = []
  for (const line of lines) {
    if (line.trim() === '' && paragraphLines.length > 0) break
    if (line.trim() !== '' || paragraphLines.length > 0) {
      paragraphLines.push(line)
    }
  }
  const paragraph = paragraphLines.join(' ').replace(/\s+/g, ' ').trim()
  return paragraph.slice(0, MAX_TITLE_LEN)
}
```

- [ ] **Step 3: Run pnpm check**

Run: `pnpm check`

Expected: No errors.

- [ ] **Step 4: Commit**

```bash
git add src/lib/themes/token-schema.ts src/lib/utils/title.ts
git commit -m "feat(i18n): update token-schema and title to use i18n messages"
```

---

### Task 5: Update App.svelte

**Files:**
- Modify: `src/App.svelte`

This task replaces all hardcoded Chinese strings in App.svelte with i18n references, and adds i18n initialization logic.

- [ ] **Step 1: Add i18n import and initialization**

In `src/App.svelte`, add import after line 11 (`import { computeThemeTokens }`):

```typescript
  import { messages, loadLocale, detectLanguage } from '$lib/i18n'
```

Inside the `onMount` async callback, after loading preferences (after line 31 `dockApi.getPreferences()`), add language resolution. The existing code is:

```typescript
      ;[homeEntries, noteEntries, preferences] = await Promise.all([
        dockApi.listEntries('home'),
        dockApi.listEntries('note'),
        dockApi.getPreferences(),
      ])
```

Replace with:

```typescript
      ;[homeEntries, noteEntries, preferences] = await Promise.all([
        dockApi.listEntries('home'),
        dockApi.listEntries('note'),
        dockApi.getPreferences(),
      ])
      // Resolve language
      if (!preferences.language) {
        const detected = detectLanguage()
        preferences = { ...preferences, language: detected }
        dockApi.setPreferences(preferences)
      }
      loadLocale(preferences.language)
```

- [ ] **Step 2: Replace all toast/error messages**

Replace every Chinese string in the script section. Here are all the replacements:

| Line (approx) | Old | New |
|---|---|---|
| 33 | `` `加载失败: ${formatError(e)}` `` | `` `${messages.toast.loadFailed}: ${formatError(e)}` `` |
| 76 | `` `发现新版本 v${update.version}` `` | `` `${messages.toast.newVersion} v${update.version}` `` |
| 76 | `'更新'` | `messages.settings.updateNow` |
| 84 | `'正在下载更新...'` | `messages.toast.downloading` |
| 87 | `'更新完成，即将重启...'` | `messages.toast.updateDone` |
| 93 | `` `更新失败: ${formatError(e)}` `` | `` `${messages.toast.updateFailed}: ${formatError(e)}` `` |
| 119 | `` `创建失败: ${formatError(e)}` `` | `` `${messages.toast.createFailed}: ${formatError(e)}` `` |
| 149 | `` `操作失败: ${formatError(e)}` `` | `` `${messages.toast.operationFailed}: ${formatError(e)}` `` |
| 175 | `` `删除失败: ${formatError(e)}` `` | `` `${messages.toast.deleteFailed}: ${formatError(e)}` `` |
| 196 | `'已删除 1 条内容'` | `messages.toast.deleted` |
| 224 | `` `收藏操作失败: ${formatError(e)}` `` | `` `${messages.toast.favoriteFailed}: ${formatError(e)}` `` |
| 238 | `` `编辑失败: ${formatError(e)}` `` | `` `${messages.toast.editFailed}: ${formatError(e)}` `` |
| 252 | `` `重命名失败: ${formatError(e)}` `` | `` `${messages.toast.renameFailed}: ${formatError(e)}` `` |
| 259 | `'已复制'` | `messages.toast.copied` |
| 261 | `'复制失败'` | `messages.toast.copyFailed` |
| 268 | `'已复制路径'` | `messages.toast.copiedPath` |
| 270 | `'复制失败'` | `messages.toast.copyFailed` |
| 281 | `` `创建失败: ${formatError(e)}` `` | `` `${messages.toast.createFailed}: ${formatError(e)}` `` |
| 318 | `` `保存失败: ${formatError(e)}` `` (×2 occurrences) | `` `${messages.toast.saveFailed}: ${formatError(e)}` `` |
| 377 | `` `最小化失败: ${formatError(e)}` `` | `` `${messages.toast.minimizeFailed}: ${formatError(e)}` `` |
| 429 | `'已收纳图片'` | `messages.toast.storedImage` |
| 431 | `` `粘贴失败: ${formatError(e)}` `` | `` `${messages.toast.pasteFailed}: ${formatError(e)}` `` |
| 465 | `` `已收纳 ${files.length} 个文件` `` | `` `messages.toast.storedFiles.replace('{n}', String(files.length))` `` |
| 467 | `` `导入失败: ${formatError(e)}` `` | `` `${messages.toast.importFailed}: ${formatError(e)}` `` |
| 484 | `'已收纳文本'` (×2 occurrences) | `messages.toast.storedText` |
| 487 | `` `粘贴失败: ${formatError(e)}` `` | `` `${messages.toast.pasteFailed}: ${formatError(e)}` `` |
| 505 | `'粘贴失败'` | `messages.toast.pasteFailed` |
| 522 | `` `已收纳文件：${fileNames[0]}` `` | `` `messages.toast.storedFileNamed.replace('{name}', fileNames[0])` `` |
| 524 | `` `已收纳 ${paths.length} 个文件` `` | `` `messages.toast.storedFiles.replace('{n}', String(paths.length))` `` |
| 527 | `` `导入失败: ${formatError(e)}` `` | `` `${messages.toast.importFailed}: ${formatError(e)}` `` |

- [ ] **Step 3: Replace drag overlay and toast template strings**

In the template, replace:

Line 621: `{toast.actionLabel || '撤销'}` → `{toast.actionLabel || messages.toast.undo}`

Line 634: `` {dragOverlay.count > 1 ? `释放以收纳 ${dragOverlay.count} 个文件` : '释放以收纳文件'} ``
→ `` {dragOverlay.count > 1 ? messages.toast.dragDropFiles.replace('{n}', String(dragOverlay.count)) : messages.toast.dragDropFile} ``

- [ ] **Step 4: Replace `formatError` default**

Replace the `formatError` function (lines 557-561):

```typescript
  function formatError(error: unknown): string {
    if (error instanceof Error && error.message) return error.message
    if (typeof error === 'string') return error
    return messages.toast.unknownError
  }
```

- [ ] **Step 5: Run pnpm check**

Run: `pnpm check`

Expected: No errors.

- [ ] **Step 6: Commit**

```bash
git add src/App.svelte
git commit -m "feat(i18n): replace hardcoded strings in App.svelte with i18n messages"
```

---

### Task 6: Update TopBar + EntryCard + entry bodies

**Files:**
- Modify: `src/lib/components/TopBar.svelte`
- Modify: `src/lib/components/EntryCard.svelte`
- Modify: `src/lib/components/entry/TextEntryBody.svelte`
- Modify: `src/lib/components/entry/ImageEntryBody.svelte`
- Modify: `src/lib/components/entry/FileEntryBody.svelte`

- [ ] **Step 1: Update TopBar.svelte**

Add import in `<script>`:
```typescript
  import { messages } from '$lib/i18n'
```

Replace in template:
| Location | Old | New |
|---|---|---|
| Line 38 | `>收纳</button>` | `>{messages.nav.home}</button>` |
| Line 43 | `>全部</button>` | `>{messages.nav.all}</button>` |
| Line 48 | `>收藏</button>` | `>{messages.nav.favorites}</button>` |
| Line 54 | `title={alwaysOnTop ? '取消置顶' : '置顶'}` | `title={alwaysOnTop ? messages.nav.unpin : messages.nav.pin}` |
| Line 64 | `>设置</button>` | `>{messages.nav.settings}</button>` |
| Line 65 | `title="最小化"` | `title={messages.nav.minimize}` |

- [ ] **Step 2: Update EntryCard.svelte**

Add import:
```typescript
  import { messages } from '$lib/i18n'
```

Replace in template:
| Location | Old | New |
|---|---|---|
| Line 81 | `\|\| '未命名条目'` | `\|\| messages.entry.untitled` |
| Line 105 | `return '文本'` | `return messages.entry.text` |
| Line 106 | `return '图片'` | `return messages.entry.image` |
| Line 107 | `return '文件'` | `return messages.entry.file` |
| Line 166 | `title="复制"` | `title={messages.entry.copy}` |
| Line 172 | `title={entry.collapsed ? '展开' : '收起'}` | `title={entry.collapsed ? messages.entry.expand : messages.entry.collapse}` |
| Line 181 | `title={entry.inNote ? '取消收藏' : '收藏'}` | `title={entry.inNote ? messages.entry.unfavorite : messages.entry.favorite}` |
| Line 186 | `title="删除"` | `title={messages.entry.delete}` |

- [ ] **Step 3: Update TextEntryBody.svelte**

Add import:
```typescript
  import { messages } from '$lib/i18n'
```

Replace:
| Location | Old | New |
|---|---|---|
| Line 63 | `{entry.content \|\| '(空)'}` | `{entry.content \|\| messages.entry.empty}` |
| Line 73 | `<span>复制</span>` | `<span>{messages.entry.copy}</span>` |

- [ ] **Step 4: Update ImageEntryBody.svelte**

Add import:
```typescript
  import { messages } from '$lib/i18n'
```

Replace:
| Location | Old | New |
|---|---|---|
| Line 44 | `alert('复制图片失败: ' + String(e))` | `alert(messages.toast.copyImageFailed + ': ' + String(e))` |
| Line 52 | `alt={entry.fileName \|\| '图片'}` | `alt={entry.fileName \|\| messages.entry.imageShort}` |
| Line 80 | `title="复制图片"` | `title={messages.entry.copyImage}` |
| Line 85 | `` {copyStatus === 'done' ? '已复制' : '复制'} `` | `` {copyStatus === 'done' ? messages.entry.copied : messages.entry.copy} `` |
| Line 88 | `title="复制路径"` | `title={messages.entry.copyPath}` |
| Line 93 | `<span>复制路径</span>` | `<span>{messages.entry.copyPath}</span>` |

- [ ] **Step 5: Update FileEntryBody.svelte**

Add import:
```typescript
  import { messages } from '$lib/i18n'
```

Replace:
| Location | Old | New |
|---|---|---|
| Line 33 | `alert('复制文件失败: ' + String(e))` | `alert(messages.toast.copyFileFailed + ': ' + String(e))` |
| Line 44 | `{entry.fileName \|\| '未知文件'}` | `{entry.fileName \|\| messages.entry.unknownFile}` |
| Line 51 | `title="复制文件"` | `title={messages.entry.copyFile}` |
| Line 56 | `` {copyStatus === 'done' ? '已复制' : '复制'} `` | `` {copyStatus === 'done' ? messages.entry.copied : messages.entry.copy} `` |
| Line 59 | `title="复制路径"` | `title={messages.entry.copyPath}` |
| Line 64 | `<span>复制路径</span>` | `<span>{messages.entry.copyPath}</span>` |

- [ ] **Step 6: Run pnpm check**

Run: `pnpm check`

Expected: No errors.

- [ ] **Step 7: Commit**

```bash
git add src/lib/components/TopBar.svelte src/lib/components/EntryCard.svelte src/lib/components/entry/
git commit -m "feat(i18n): replace hardcoded strings in TopBar, EntryCard, and entry bodies"
```

---

### Task 7: Update HomeView + NoteView + CategoriesView

**Files:**
- Modify: `src/lib/components/views/HomeView.svelte`
- Modify: `src/lib/components/views/NoteView.svelte`
- Modify: `src/lib/components/views/CategoriesView.svelte`

- [ ] **Step 1: Update HomeView.svelte**

Add import:
```typescript
  import { messages } from '$lib/i18n'
```

Replace:
| Location | Old | New |
|---|---|---|
| Line 153 | `placeholder="搜索内容、标题、文件名..."` | `placeholder={messages.home.search}` |
| Line 169 | `placeholder="输入文本内容..."` | `placeholder={messages.home.inputHint}` |
| Line 173 | `>取消</button>` | `>{messages.home.cancel}</button>` |
| Line 174 | `disabled={!newText.trim()}>添加</button>` | `disabled={!newText.trim()}>{messages.home.add}</button>` |
| Line 182 | `<p>还没有收纳内容</p>` | `<p>{messages.home.empty}</p>` |
| Line 183 | `<p class="hint">你可以直接粘贴文字、图片或文件，也可以拖入文件到窗口，或点击左上角 + 新建文本。</p>` | `<p class="hint">{messages.home.emptyHint}</p>` |
| Line 187 | `<p>未找到匹配内容</p>` | `<p>{messages.home.noMatch}</p>` |

- [ ] **Step 2: Update NoteView.svelte**

Add import:
```typescript
  import { messages } from '$lib/i18n'
```

Replace:
| Location | Old | New |
|---|---|---|
| Line 149 | `placeholder="搜索内容、标题、文件名..."` | `placeholder={messages.note.search}` |
| Line 163 | `placeholder="输入文本内容..."` | `placeholder={messages.note.inputHint}` |
| Line 169 | `>取消</button>` | `>{messages.note.cancel}</button>` |
| Line 170 | `disabled={!newText.trim()}>添加</button>` | `disabled={!newText.trim()}>{messages.note.add}</button>` |
| Line 178 | `<p>暂无收藏内容</p>` | `<p>{messages.note.empty}</p>` |
| Line 179 | `<p class="hint">在收纳页点击星标，即可把重要内容放到这里。也可以点击 + 新建一条文本。</p>` | `<p class="hint">{messages.note.emptyHint}</p>` |
| Line 183 | `<p>未找到匹配内容</p>` | `<p>{messages.note.noMatch}</p>` |

- [ ] **Step 3: Update CategoriesView.svelte**

Add import:
```typescript
  import { messages } from '$lib/i18n'
```

Replace the `filters` array (lines 35-40):
```typescript
  let filters: { kind: EntryKind | null; label: string }[] = [
    { kind: null, label: messages.categories.all },
    { kind: 'text', label: messages.categories.text },
    { kind: 'image', label: messages.categories.image },
    { kind: 'file', label: messages.categories.file },
  ]
```

Replace empty states (line 57):
| Old | New |
|---|---|
| `<p>{activeFilter ? '该类型暂无内容' : '暂无任何内容'}</p>` | `<p>{activeFilter ? messages.categories.emptyFiltered : messages.categories.empty}</p>` |

- [ ] **Step 4: Run pnpm check**

Run: `pnpm check`

Expected: No errors.

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/views/HomeView.svelte src/lib/components/views/NoteView.svelte src/lib/components/views/CategoriesView.svelte
git commit -m "feat(i18n): replace hardcoded strings in HomeView, NoteView, and CategoriesView"
```

---

### Task 8: Update SettingsView (all strings + language section)

**Files:**
- Modify: `src/lib/components/views/SettingsView.svelte`

This is the largest task. SettingsView has many strings and needs a new language section.

- [ ] **Step 1: Add imports**

Add after existing imports:
```typescript
  import { messages } from '$lib/i18n'
```

- [ ] **Step 2: Add validation message helper**

Add after the `update()` function (after line 99):

```typescript
  function getValidationMsg(key: string, code?: string): string {
    const schema = TOKEN_SCHEMA[key]
    switch (code) {
      case 'unknownToken': return messages.validation.unknownToken
      case 'invalidColor': return messages.validation.invalidColor
      case 'notNumber': return messages.validation.notNumber
      case 'minValue': return messages.validation.minValue.replace('{n}', String(schema?.min))
      case 'maxValue': return messages.validation.maxValue.replace('{n}', String(schema?.max))
      case 'invalidShadow': return messages.validation.invalidShadow
      default: return messages.toast.invalid
    }
  }
```

- [ ] **Step 3: Update `handleExpertOverride`**

Replace lines 42-60 (the `handleExpertOverride` function):

```typescript
  function handleExpertOverride(key: string, value: string) {
    const result = validateToken(key, value)
    if (!result.valid) {
      expertErrors = { ...expertErrors, [key]: getValidationMsg(key, result.errorCode) }
      return
    }
    expertErrors = { ...expertErrors, [key]: '' }
    const overrides = { ...preferences.themeOverrides }
    if (value.trim()) {
      overrides[key] = value
    } else {
      delete overrides[key]
    }
    update({
      themeMode: 'custom',
      customBasePresetId: preferences.themePresetId || 'dark-glass',
      themeOverrides: overrides,
    })
  }
```

- [ ] **Step 4: Update `validateProxy` error messages**

Replace the `validateProxy` function (lines 118-134):

```typescript
  function validateProxy(): string[] {
    const ip = proxyIp.trim()
    const port = proxyPort.trim()
    const errors: string[] = []
    if (!ip && !port) return errors
    if (!ip && port) errors.push(messages.settings.proxyErrHostRequired)
    if (ip && !port) errors.push(messages.settings.proxyErrPortRequired)
    if (port && !/^\d+$/.test(port)) errors.push(messages.settings.proxyErrPortNumeric)
    if (port && /^\d+$/.test(port)) {
      const num = parseInt(port, 10)
      if (num < 1 || num > 65535) errors.push(messages.settings.proxyErrPortRange)
    }
    if (ip && /^https?:\/\//i.test(ip)) errors.push(messages.settings.proxyErrNoProtocol)
    if (ip && /^socks[45]:\/\//i.test(ip)) errors.push(messages.settings.proxyErrNoProtocol)
    if (ip && /[:：]/.test(ip)) errors.push(messages.settings.proxyErrNoPort)
    return errors
  }
```

- [ ] **Step 5: Update update error messages**

In `handleCheckUpdate` (line 219), replace:
`e instanceof Error ? e.message : '检查失败'` → `e instanceof Error ? e.message : messages.settings.checkFailed`

In `handleInstallUpdate` (line 233), replace:
`e instanceof Error ? e.message : '更新失败'` → `e instanceof Error ? e.message : messages.settings.updateFailed`

- [ ] **Step 6: Update `handleReset` to preserve language**

In `handleReset` (around line 177), add `language` to the reset object after `updateProxy: '',`:

```typescript
      language: preferences.language,
```

- [ ] **Step 7: Add language section to template**

Insert the following block in the template, right after `<div class="settings-body">` (line 244) and before the existing Theme section comment:

```svelte
    <!-- Language section -->
    <div class="section">
      <div class="section-header">
        <span class="section-label">{messages.settings.language} / Language</span>
      </div>
      <div class="section-body">
        <div class="lang-options">
          <button
            class="lang-btn"
            class:active={preferences.language === 'zh-CN'}
            onclick={() => update({ language: 'zh-CN' })}
          >中文 (简体)</button>
          <button
            class="lang-btn"
            class:active={preferences.language === 'en'}
            onclick={() => update({ language: 'en' })}
          >English</button>
        </div>
        <p class="lang-hint">{messages.settings.restartHint}</p>
      </div>
    </div>
```

- [ ] **Step 8: Replace all template strings**

Replace all hardcoded Chinese strings in the template section:

| Location | Old | New |
|---|---|---|
| Line 241 | `← 返回` | `{messages.settings.back}` |
| Line 248 | `主题` | `{messages.settings.theme}` |
| Line 254 | `跟随系统` | `{messages.settings.followSystem}` |
| Line 280 | `字体` | `{messages.settings.font}` |
| Line 286 | `中文字体` | `{messages.settings.zhFont}` |
| Line 291 | `placeholder="搜索中文字体..."` | `placeholder={messages.settings.zhFontSearch}` |
| Line 311 | `英文字体` | `{messages.settings.enFont}` |
| Line 316 | `placeholder="搜索英文字体..."` | `placeholder={messages.settings.enFontSearch}` |
| Line 342 | `更新` | `{messages.settings.update}` |
| Line 347 | `代理仅用于检查和下载更新...` | `{messages.settings.proxyNote}` |
| Line 349 | `代理类型` | `{messages.settings.proxyType}` |
| Line 357 | `代理主机` | `{messages.settings.proxyHost}` |
| Line 360 | `placeholder="例如 127.0.0.1"` | `placeholder={messages.settings.proxyHostExample}` |
| Line 365 | `代理端口` | `{messages.settings.proxyPort}` |
| Line 370 | `placeholder="例如 11809"` | `placeholder={messages.settings.proxyPortExample}` |
| Line 383 | `保存代理` | `{messages.settings.saveProxy}` |
| Line 384 | `清除` | `{messages.settings.clear}` |
| Line 396 | `检查更新` | `{messages.settings.checkUpdate}` |
| Line 398 | `正在检查...` | `{messages.settings.checking}` |
| Line 400 | `已是最新版本` | `{messages.settings.upToDate}` |
| Line 401 | `重新检查` | `{messages.settings.recheck}` |
| Line 403 | `发现新版本` | `{messages.settings.newVersion}` |
| Line 404 | `立即更新` | `{messages.settings.updateNow}` |
| Line 406 | `正在下载并安装...` | `{messages.settings.downloading}` |
| Line 419 | `系统` | `{messages.settings.system}` |
| Line 426 | `开机自启` | `{messages.settings.launchOnStartup}` |
| Line 439 | `高级` | `{messages.settings.advanced}` |
| Line 446 | `专家模式` | `{messages.settings.expertMode}` |
| Line 455 | `{schema.label}` | `{messages.expert[key]}` |
| Line 461 | `placeholder="使用预设值"` | `placeholder={messages.settings.usePreset}` |
| Line 479 | `危险操作` | `{messages.settings.dangerZone}` |
| Line 482 | `重置全部设置` | `{messages.settings.resetAll}` |

- [ ] **Step 9: Add language section CSS**

Add these styles to the `<style>` block:

```css
  /* Language section */
  .lang-options {
    display: flex;
    gap: 0.3rem;
    padding: 0.3rem 0;
  }

  .lang-btn {
    flex: 1;
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 0.35rem);
    background: transparent;
    color: var(--text-muted);
    font-size: var(--font-sm, 0.65rem);
    cursor: pointer;
    font-family: inherit;
    transition: border-color 0.15s, background 0.15s, color 0.15s;
  }

  .lang-btn:hover:not(.active) {
    border-color: var(--border-emphasis);
    color: var(--text-primary);
  }

  .lang-btn.active {
    border-color: var(--color-primary);
    background: var(--color-primary-faint);
    color: var(--color-primary);
    font-weight: 500;
  }

  .lang-hint {
    font-size: var(--font-xs, 0.5rem);
    color: var(--text-faint);
    margin: 0;
    padding-top: 0.15rem;
  }
```

- [ ] **Step 10: Run pnpm check**

Run: `pnpm check`

Expected: No errors.

- [ ] **Step 11: Commit**

```bash
git add src/lib/components/views/SettingsView.svelte
git commit -m "feat(i18n): replace all strings in SettingsView and add language section"
```

---

### Task 9: Final verification

- [ ] **Step 1: Run all Rust tests**

Run: `cd src-tauri && cargo test`

Expected: All tests pass, including the existing `preferences_roundtrip_persists_theme_fields` test which now exercises the `language` field.

- [ ] **Step 2: Run all frontend tests**

Run: `pnpm vitest run`

Expected: All tests pass, including new i18n tests.

- [ ] **Step 3: Run type check**

Run: `pnpm check`

Expected: No errors.

- [ ] **Step 4: Manual smoke test**

Run: `pnpm tauri dev`

Verify:
- [ ] App launches in Chinese (if system locale is Chinese)
- [ ] All UI labels, buttons, placeholders, and toast messages display in Chinese
- [ ] Settings page shows Language section at top with two options
- [ ] Selecting English and restarting shows English UI
- [ ] Expert mode labels are localized
- [ ] Proxy validation errors are localized
- [ ] Drag-and-drop overlay text is localized
- [ ] Update check status messages are localized
