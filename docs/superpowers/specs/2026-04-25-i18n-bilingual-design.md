# i18n Bilingual Support Design

**Date:** 2026-04-25
**Status:** Approved

## Overview

Add Chinese (zh-CN) and English (en) bilingual support to Soma Scratchpad. Language preference is stored in existing `DockPreferences`, takes effect after restart, and defaults to system locale on first run.

## Decisions

| Decision | Choice |
|----------|--------|
| Runtime vs restart | Restart to apply |
| Storage | `language` field in `DockPreferences` |
| Default behavior | Follow system locale (`navigator.language`); fallback to English for non-Chinese/non-English |
| Settings UI location | Standalone "Language" section at top of settings page |
| i18n approach | Lightweight JSON dictionaries (zero dependencies) |

## Data Model Changes

### Rust — `DockPreferences`

Add one field:

```rust
pub language: String,  // "zh-CN" | "en", default "" (unspecified)
```

Default value is empty string `""`, meaning "not yet detected". Frontend resolves this on first load.

### TypeScript — `DockPreferences`

Add matching field:

```typescript
language: string  // 'zh-CN' | 'en'
```

### Language Detection

On app startup, if `preferences.language` is empty:

1. Read `navigator.language`
2. If starts with `"zh"` → `"zh-CN"`
3. Otherwise → `"en"`
4. Write resolved value back to preferences

Existing users upgrading will auto-detect appropriate language on first launch after upgrade.

## i18n Module

### File Structure

```
src/lib/i18n/
  types.ts              — LocaleMessages interface definition
  index.ts              — loadLocale(), detectLanguage(), exported `messages`
  locales/
    zh-CN.ts            — Chinese dictionary
    en.ts               — English dictionary
```

### Dictionary Structure

Messages are organized by component/feature as a nested typed object:

```typescript
interface LocaleMessages {
  nav: { home, all, favorites, settings, pin, unpin, minimize }
  home: { search, inputHint, cancel, add, empty, emptyHint, noMatch }
  note: { search, inputHint, cancel, add, empty, emptyHint, noMatch }
  categories: { all, text, image, file, emptyFiltered, empty }
  entry: { text, image, file, untitled, empty, copy, copied, copyPath, expand, collapse, favorite, unfavorite, delete }
  settings: { back, theme, followSystem, font, zhFont, enFont, zhFontSearch, enFontSearch, update, proxyOnlyNote, proxyType, proxyHost, proxyPort, proxyHostExample, proxyPortExample, saveProxy, clear, checkUpdate, checking, upToDate, recheck, newVersion, updateNow, downloading, updateFailed, system, launchOnStartup, advanced, expertMode, usePreset, dangerZone, resetAll, language, restartHint }
  toast: { loadFailed, newVersion, downloading, updateDone, updateFailed, createFailed, deleteFailed, favoriteFailed, editFailed, renameFailed, saveFailed, minimizeFailed, copyFailed, pasteFailed, importFailed, deleted, undo, copied, copiedPath, storedImage, storedText, storedFile, storedFiles, dragDropFile, dragDropFiles, unknownError, copyImageFailed, copyFileFailed, invalid }
  expert: Record<string, string>  // Dynamic keys: --color-primary → label
}
```

### Usage in Components

Import `messages` directly — no wrapper function needed:

```svelte
<script>
import { messages } from '$lib/i18n'
</script>
<button>{messages.nav.home}</button>
```

Benefits over `t('nav.home')`:
- Full IDE autocomplete and type checking
- Zero runtime overhead (direct property access)
- Compile-time typo detection

### Initialization

In `App.svelte` `onMount`:

1. Load preferences (existing)
2. If `preferences.language` is empty, call `detectLanguage()`, resolve, and save back
3. Call `loadLocale(preferences.language)` to set global `messages`
4. Render proceeds using loaded dictionary

Since language takes effect at restart, `messages` is assigned once and never changes during session.

## Settings UI

### Language Section

Positioned as the first section in `SettingsView.svelte`, above the Theme section.

- Section header: `语言 / Language` (always bilingual, unaffected by current language)
- No collapse chevron — always expanded
- Two radio-style option buttons: `中文 (简体)` and `English`
- Active option has highlighted style
- Click triggers `onChange({ ...preferences, language: selectedLang })` via existing save chain
- Bottom hint: `语言将在重启后生效` / `Language will take effect after restart`

## Files Modified

### New Files
- `src/lib/i18n/types.ts`
- `src/lib/i18n/index.ts`
- `src/lib/i18n/locales/zh-CN.ts`
- `src/lib/i18n/locales/en.ts`

### Modified Files
- `src/lib/types/dock.ts` — add `language` field
- `src-tauri/src/models/preferences.rs` — add `language` field + default
- `src/App.svelte` — i18n init + replace ~20 hardcoded strings
- `src/lib/components/TopBar.svelte` — nav labels, tooltips
- `src/lib/components/views/HomeView.svelte` — search, form, empty states
- `src/lib/components/views/NoteView.svelte` — search, form, empty states
- `src/lib/components/views/CategoriesView.svelte` — filter labels, empty states
- `src/lib/components/views/SettingsView.svelte` — all labels, buttons, status text + language section
- `src/lib/components/EntryCard.svelte` — kind badges, tooltips, default title
- `src/lib/components/entry/TextEntryBody.svelte` — copy button, empty text
- `src/lib/components/entry/ImageEntryBody.svelte` — copy/copied/copyPath
- `src/lib/components/entry/FileEntryBody.svelte` — copy/copied/copyPath, unknown file
- `src/lib/themes/token-schema.ts` — labels sourced from i18n

## Migration

No database migration needed. The new `language` field defaults to `""` in Rust. Existing SQLite data is unaffected — the field is added to the preferences struct only. When Rust encounters an empty string for `language` (from old data missing the field), it preserves `""`, and the frontend resolves it via system locale detection.
