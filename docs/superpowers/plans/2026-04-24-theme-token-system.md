# Theme & Token System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace all hardcoded `rgba()` values across 11 Svelte components with a 30-token CSS variable system, add 3 preset themes (dark/light-matte/light-frosted), system dark/light auto-detection, and a redesigned settings page with expert mode.

**Architecture:** Rust `DockPreferences` persists `theme_mode`/`theme_overrides`/layout fields in SQLite key-value store. Frontend `computeThemeTokens()` merges preset → layout presets → overrides into a flat CSS variable map, applied via `$effect() → root.style.setProperty()`. Components reference `var(--token)` instead of raw colors.

**Tech Stack:** Tauri (Rust) + Svelte 5 + TypeScript + vitest + CSS custom properties

**Spec:** `docs/superpowers/specs/2026-04-23-theme-token-system-design.md`

---

## File Map

### New files
| File | Responsibility |
|---|---|
| `src/lib/themes/types.ts` | ThemePreset, SpacingPreset, RadiusPreset types |
| `src/lib/themes/presets.ts` | 3 preset theme token maps (18 color + 1 shadow each) |
| `src/lib/themes/layout.ts` | Spacing/radius preset tables |
| `src/lib/themes/engine.ts` | `computeThemeTokens()` pure function |
| `src/lib/themes/__tests__/engine.test.ts` | Unit tests for engine |
| `src/lib/themes/token-schema.ts` | TOKEN_SCHEMA map for expert mode validation |

### Modified files
| File | What changes |
|---|---|
| `src-tauri/src/models/preferences.rs` | Add/remove DockPreferences fields |
| `src-tauri/src/scratchpad/preferences.rs` | Save/load new fields, remove old ones |
| `src/lib/types/dock.ts` | Update DockPreferences interface |
| `src/lib/api/dock.ts` | No changes needed (generic setPreferences) |
| `src/app.css` | Remove old CSS vars, add semantic token references |
| `src/App.svelte` | Theme application `$effect`, `systemDark` reactive state, debounce save, toast colors |
| `src/lib/components/TopBar.svelte` | Replace hardcoded colors with var() |
| `src/lib/components/EntryCard.svelte` | Replace hardcoded colors with var() |
| `src/lib/components/views/HomeView.svelte` | Replace hardcoded colors |
| `src/lib/components/views/NoteView.svelte` | Replace hardcoded colors |
| `src/lib/components/views/CategoriesView.svelte` | Replace hardcoded colors |
| `src/lib/components/views/SettingsView.svelte` | Complete redesign |
| `src/lib/components/entry/TextEntryBody.svelte` | Replace hardcoded colors |
| `src/lib/components/entry/ImageEntryBody.svelte` | Replace hardcoded colors |
| `src/lib/components/entry/FileEntryBody.svelte` | Replace hardcoded colors |
| `src/lib/components/MinimizedTab.svelte` | No token changes (transparent) |

**Note:** `vitest` (v4.1.4) is already installed in `package.json` and configured in `vite.config.ts` with `@sveltejs/vite-plugin-svelte`. Test script is `test:unit` → `vitest run`. No vitest setup needed.

---

## Task 1: Create theme types

**Files:**
- Create: `src/lib/themes/types.ts`

**Prerequisite:** vitest v4.1.4 is already installed (`package.json` devDependencies). `vite.config.ts` already has `test: { include: ["src/**/*.test.ts"] }`. Test script is `pnpm test:unit`. No setup needed.

- [ ] **Step 1: Create `src/lib/themes/types.ts`**

```typescript
export type ThemeMode = 'system' | 'preset' | 'custom'
export type SpacingPreset = 'compact' | 'normal' | 'spacious'
export type RadiusPreset = 'sharp' | 'normal' | 'round'

export interface ThemePreset {
  id: string
  name: string
  tokens: Record<string, string>
}
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/themes/types.ts
git commit -m "feat: add theme type definitions"
```

---

## Task 2: Create theme preset data

**Files:**
- Create: `src/lib/themes/presets.ts`
- Create: `src/lib/themes/layout.ts`

- [ ] **Step 1: Create `src/lib/themes/presets.ts`**

This file contains the full 18 color + 1 shadow token values for each preset. Values taken directly from spec section 2.

```typescript
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
    '--surface-0': 'rgba(42, 53, 72, 0.85)',
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
    '--surface-0': 'rgba(250, 250, 252, 0.78)',
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
    '--surface-0': 'rgba(245, 243, 238, 0.78)',
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
```

- [ ] **Step 2: Create `src/lib/themes/layout.ts`**

```typescript
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
```

- [ ] **Step 3: Commit**

```bash
git add src/lib/themes/presets.ts src/lib/themes/layout.ts
git commit -m "feat: add theme preset definitions and layout token tables"
```

---

## Task 3: Write failing tests for computeThemeTokens

**Files:**
- Create: `src/lib/themes/__tests__/engine.test.ts`

- [ ] **Step 1: Write failing tests**

```typescript
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
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd E:/codex-prj/Scratchpad && pnpm test:unit -- src/lib/themes/__tests__/engine.test.ts
```

Expected: FAIL (module not found — engine.ts doesn't exist yet)

- [ ] **Step 3: Commit the test**

```bash
git add src/lib/themes/__tests__/engine.test.ts
git commit -m "test: add failing tests for computeThemeTokens"
```

---

## Task 4: Implement computeThemeTokens engine

**Files:**
- Create: `src/lib/themes/engine.ts`

- [ ] **Step 1: Implement engine**

```typescript
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
```

- [ ] **Step 2: Run tests**

```bash
cd E:/codex-prj/Scratchpad && pnpm test:unit -- src/lib/themes/__tests__/engine.test.ts
```

Expected: ALL PASS

- [ ] **Step 3: Commit**

```bash
git add src/lib/themes/engine.ts
git commit -m "feat: implement computeThemeTokens engine"
```

---

## Task 5: Update Rust DockPreferences model

**Files:**
- Modify: `src-tauri/src/models/preferences.rs`

- [ ] **Step 1: Update the struct**

Replace the entire `DockPreferences` struct and `Default` impl in `src-tauri/src/models/preferences.rs`:

```rust
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::system::fonts::system_font_defaults;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DockPreferences {
    // Theme
    pub theme_mode: String,                          // "system" | "preset" | "custom"
    pub theme_preset_id: String,                     // "dark-glass" | "light-matte" | "light-frosted"
    pub custom_base_preset_id: String,
    pub theme_overrides: HashMap<String, String>,

    // Layout
    pub ui_text_size_px: f64,                        // default 12
    pub content_text_size_px: f64,                   // default 14
    pub spacing_preset: String,                      // "compact" | "normal" | "spacious"
    pub radius_preset: String,                       // "sharp" | "normal" | "round"

    // Window geometry
    pub dock_position_x: f64,
    pub dock_position_y: f64,
    pub dock_width: f64,
    pub dock_height: f64,
    pub dock_edge_anchor: String,
    pub dock_minimized: bool,

    // Fonts
    pub font_family_zh: String,
    pub font_family_en: String,

    // System
    pub launch_on_startup: bool,
    pub update_proxy: String,
}

impl Default for DockPreferences {
    fn default() -> Self {
        let (sys_font, _sys_size) = system_font_defaults();
        Self {
            theme_mode: "system".to_string(),
            theme_preset_id: "dark-glass".to_string(),
            custom_base_preset_id: String::new(),
            theme_overrides: HashMap::new(),
            ui_text_size_px: 12.0,
            content_text_size_px: 14.0,
            spacing_preset: "normal".to_string(),
            radius_preset: "normal".to_string(),
            dock_position_x: 40.0,
            dock_position_y: 40.0,
            dock_width: 360.0,
            dock_height: 640.0,
            dock_edge_anchor: "right".to_string(),
            dock_minimized: false,
            font_family_zh: sys_font.clone(),
            font_family_en: sys_font,
            launch_on_startup: false,
            update_proxy: String::new(),
        }
    }
}
```

- [ ] **Step 2: Verify Rust compiles**

```bash
cd E:/codex-prj/Scratchpad/src-tauri && cargo check
```

Expected: Compile errors in `preferences.rs` save/load (field mismatch) — that's Task 6.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/models/preferences.rs
git commit -m "refactor: update DockPreferences struct for theme system"
```

---

## Task 6: Update Rust preferences save/load

**Files:**
- Modify: `src-tauri/src/scratchpad/preferences.rs`

- [ ] **Step 1: Update save_preferences**

Replace the key-value pairs in `save_preferences`. `theme_overrides` HashMap is serialized as JSON string.

```rust
use rusqlite::{params, Connection};
use serde_json;

use crate::models::preferences::DockPreferences;
use crate::storage::error::StorageResult;

pub fn save_preferences(conn: &mut Connection, prefs: &DockPreferences) -> StorageResult<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM preferences", [])?;

    let overrides_json = serde_json::to_string(&prefs.theme_overrides)
        .unwrap_or_else(|_| "{}".to_string());

    for (key, value) in [
        ("theme_mode", prefs.theme_mode.clone()),
        ("theme_preset_id", prefs.theme_preset_id.clone()),
        ("custom_base_preset_id", prefs.custom_base_preset_id.clone()),
        ("theme_overrides", overrides_json),
        ("ui_text_size_px", prefs.ui_text_size_px.to_string()),
        ("content_text_size_px", prefs.content_text_size_px.to_string()),
        ("spacing_preset", prefs.spacing_preset.clone()),
        ("radius_preset", prefs.radius_preset.clone()),
        ("dock_position_x", prefs.dock_position_x.to_string()),
        ("dock_position_y", prefs.dock_position_y.to_string()),
        ("dock_width", prefs.dock_width.to_string()),
        ("dock_height", prefs.dock_height.to_string()),
        ("dock_edge_anchor", prefs.dock_edge_anchor.clone()),
        ("dock_minimized", prefs.dock_minimized.to_string()),
        ("font_family_zh", prefs.font_family_zh.clone()),
        ("font_family_en", prefs.font_family_en.clone()),
        ("launch_on_startup", prefs.launch_on_startup.to_string()),
        ("update_proxy", prefs.update_proxy.clone()),
    ] {
        tx.execute(
            "INSERT INTO preferences(key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
    }
    tx.commit()?;
    Ok(())
}
```

- [ ] **Step 2: Update load_preferences**

```rust
pub fn load_preferences(conn: &Connection) -> StorageResult<DockPreferences> {
    let mut stmt = conn.prepare("SELECT key, value FROM preferences")?;
    let raw: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let map: std::collections::HashMap<String, String> = raw.into_iter().collect();

    let mut prefs = DockPreferences::default();

    if let Some(v) = map.get("theme_mode") {
        prefs.theme_mode = v.clone();
    }
    if let Some(v) = map.get("theme_preset_id") {
        prefs.theme_preset_id = v.clone();
    }
    if let Some(v) = map.get("custom_base_preset_id") {
        prefs.custom_base_preset_id = v.clone();
    }
    if let Some(v) = map.get("theme_overrides") {
        prefs.theme_overrides = serde_json::from_str(v)
            .unwrap_or_default();
    }
    if let Some(v) = map.get("ui_text_size_px") {
        prefs.ui_text_size_px = v.parse().unwrap_or(prefs.ui_text_size_px);
    }
    if let Some(v) = map.get("content_text_size_px") {
        prefs.content_text_size_px = v.parse().unwrap_or(prefs.content_text_size_px);
    }
    if let Some(v) = map.get("spacing_preset") {
        prefs.spacing_preset = v.clone();
    }
    if let Some(v) = map.get("radius_preset") {
        prefs.radius_preset = v.clone();
    }
    if let Some(v) = map.get("dock_position_x") {
        prefs.dock_position_x = v.parse().unwrap_or(prefs.dock_position_x);
    }
    if let Some(v) = map.get("dock_position_y") {
        prefs.dock_position_y = v.parse().unwrap_or(prefs.dock_position_y);
    }
    if let Some(v) = map.get("dock_width") {
        prefs.dock_width = v.parse().unwrap_or(prefs.dock_width);
    }
    if let Some(v) = map.get("dock_height") {
        prefs.dock_height = v.parse().unwrap_or(prefs.dock_height);
    }
    if let Some(v) = map.get("dock_edge_anchor") {
        prefs.dock_edge_anchor = v.clone();
    }
    if let Some(v) = map.get("dock_minimized") {
        prefs.dock_minimized = v.parse().unwrap_or(false);
    }
    if let Some(v) = map.get("font_family_zh") {
        prefs.font_family_zh = v.clone();
    }
    if let Some(v) = map.get("font_family_en") {
        prefs.font_family_en = v.clone();
    }
    if let Some(v) = map.get("launch_on_startup") {
        prefs.launch_on_startup = v.parse().unwrap_or(false);
    }
    if let Some(v) = map.get("update_proxy") {
        prefs.update_proxy = v.clone();
    }

    Ok(prefs)
}
```

Keep `resolve_font` and `load_preferences_with_fonts` unchanged (they only touch font fields which still exist).

- [ ] **Step 3: Update existing tests**

In the test module at the bottom of the same file, update the roundtrip test:

```rust
#[test]
fn preferences_roundtrip_persists_theme_fields() {
    let mut conn = Connection::open_in_memory().unwrap();
    ensure_dock_schema(&mut conn).unwrap();

    let mut prefs = DockPreferences::default();
    prefs.theme_mode = "custom".to_string();
    prefs.custom_base_preset_id = "dark-glass".to_string();
    prefs.theme_overrides.insert("--color-primary".to_string(), "#ff0000".to_string());
    prefs.ui_text_size_px = 14.0;
    prefs.spacing_preset = "compact".to_string();

    save_preferences(&mut conn, &prefs).unwrap();
    let loaded = load_preferences(&conn).unwrap();

    assert_eq!(loaded.theme_mode, "custom");
    assert_eq!(loaded.custom_base_preset_id, "dark-glass");
    assert_eq!(loaded.theme_overrides.get("--color-primary"), Some(&"#ff0000".to_string()));
    assert_eq!(loaded.ui_text_size_px, 14.0);
    assert_eq!(loaded.spacing_preset, "compact");
}
```

- [ ] **Step 4: Run Rust tests**

```bash
cd E:/codex-prj/Scratchpad/src-tauri && cargo test
```

Expected: ALL PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/scratchpad/preferences.rs
git commit -m "feat: update preferences save/load for theme fields"
```

---

## Task 7: Update TypeScript types

**Files:**
- Modify: `src/lib/types/dock.ts`

- [ ] **Step 1: Replace DockPreferences interface**

```typescript
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
}
```

- [ ] **Step 2: Verify TypeScript**

```bash
cd E:/codex-prj/Scratchpad && pnpm check
```

Expected: Errors in App.svelte and SettingsView (they reference old fields) — that's expected, will fix in later tasks.

- [ ] **Step 3: Commit**

```bash
git add src/lib/types/dock.ts
git commit -m "refactor: update DockPreferences TypeScript type for theme system"
```

---

## Task 8: Wire theme application in App.svelte + app.css

**Files:**
- Modify: `src/app.css`
- Modify: `src/App.svelte`

This is the critical wiring task that connects the engine to CSS.

- [ ] **Step 1: Rewrite `src/app.css`**

Remove old CSS variable definitions and replace with semantic token references:

```css
:root {
  font-family: var(--font-family-en, "Segoe UI"), var(--font-family-zh, "Microsoft YaHei"), sans-serif;
  line-height: 1.5;
  font-weight: 400;
  color: var(--text-primary);
  font-size: 15px;
  background: transparent;
  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  min-width: 260px;
  height: 100vh;
  overflow: hidden;
  background: transparent;
}

#app {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--surface-0);
  backdrop-filter: blur(24px);
  border: 1px solid var(--border-emphasis);
  box-shadow: var(--shadow-default);
  border-radius: 0.5rem;
}

::-webkit-scrollbar {
  width: 3px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background: color-mix(in srgb, var(--text-muted) 30%, transparent);
  border-radius: 2px;
}
```

Note: Semantic tokens (like `--surface-0`) are set at runtime by the theme engine. Components reference semantic tokens directly via `var()`. The `#app.minimized-mode` and `body.minimized-mode` CSS has been **removed** — `MinimizedApp.svelte` handles transparency independently when the app is minimized to tab.

- [ ] **Step 2: Add `systemDark` reactive state + theme `$effect` in App.svelte**

Add a `$state` variable for system dark mode detection, and a separate synchronous `onMount` for the `matchMedia` listener (not async — so the cleanup return works). Replace the existing `$effect` that sets CSS variables (around line 80-90) with:

```typescript
import { computeThemeTokens } from '$lib/themes/engine'

// ... inside the component script ...

// Reactive system dark mode — separate from main async onMount
let systemDark = $state(window.matchMedia('(prefers-color-scheme: dark)').matches)

// Synchronous onMount for matchMedia listener — cleanup function works correctly
onMount(() => {
  const mq = window.matchMedia('(prefers-color-scheme: dark)')
  function onSystemThemeChange(e: MediaQueryListEvent) {
    systemDark = e.matches
  }
  mq.addEventListener('change', onSystemThemeChange)
  return () => mq.removeEventListener('change', onSystemThemeChange)
})

// Theme application — reacts to both preferences and systemDark
$effect(() => {
  if (!preferences) return
  const tokens = computeThemeTokens(preferences, systemDark)
  const root = document.documentElement.style
  for (const [key, value] of Object.entries(tokens)) {
    root.setProperty(key, value)
  }
  // Non-token preferences
  root.setProperty('--font-family-zh', preferences.fontFamilyZh)
  root.setProperty('--font-family-en', preferences.fontFamilyEn)
})
```

Remove the old CSS variable assignments for `--dock-bg-opacity`, `--dock-bg-color`, `--dock-text-size`, `--dock-text-color`, `--dock-font-zh`, `--dock-font-en`, `--entry-surface-opacity`.

- [ ] **Step 3: Commit**

```bash
git add src/app.css src/App.svelte
git commit -m "feat: wire theme engine to CSS variables in App.svelte"
```

---

## Task 9: Migrate component styles batch 1 — EntryCard, TopBar

**Files:**
- Modify: `src/lib/components/EntryCard.svelte`
- Modify: `src/lib/components/TopBar.svelte`

For each component, replace all hardcoded `rgba(...)` / `#hex` values with `var(--token)` references using the mapping table from the spec section 1.

- [ ] **Step 1: Migrate EntryCard.svelte `<style>` block**

Replace the entire `<style>` block. Key replacements:
- `rgba(255, 255, 255, 0.04)` → `var(--surface-1)`
- `rgba(148, 163, 184, 0.1)` → `var(--border-default)`
- `rgba(125, 211, 252, 0.12)` / `0.2` / `0.85` → `var(--color-primary-faint)` / `var(--color-primary)` etc.
- `rgba(192, 132, 252, ...)` → `var(--color-info)` based
- `rgba(74, 222, 128, ...)` → `var(--color-file)` based
- `rgba(251, 191, 36, ...)` → `var(--color-accent)` based
- `rgba(248, 113, 113, ...)` → `var(--color-danger)` based
- All `font-size` hardcoded values → `var(--font-xs)`, `var(--font-sm)`, `var(--font-md)` per mapping
- `var(--dock-text-color)` → `var(--text-primary)` in all `color-mix()` calls
- `0.2rem` / `0.25rem` / `0.35rem` → `var(--space-sm)` / `var(--space-md)` etc.
- `0.2rem` / `0.25rem` / `0.5rem` border-radius → `var(--radius-sm)` / `var(--radius-md)` / `var(--radius-lg)`

- [ ] **Step 2: Migrate TopBar.svelte `<style>` block**

Replace:
- `rgba(148, 163, 184, 0.08)` → `var(--border-subtle)`
- `color-mix(in srgb, var(--dock-text-color) ...)` → `color-mix(in srgb, var(--text-primary) ...)`
- `rgba(125, 211, 252, 0.9)` → `var(--color-primary)`
- `0.65rem` font → `var(--font-md)`

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/EntryCard.svelte src/lib/components/TopBar.svelte
git commit -m "feat: migrate EntryCard and TopBar to CSS token system"
```

---

## Task 10: Migrate component styles batch 2 — HomeView, NoteView, CategoriesView

**Files:**
- Modify: `src/lib/components/views/HomeView.svelte`
- Modify: `src/lib/components/views/NoteView.svelte`
- Modify: `src/lib/components/views/CategoriesView.svelte`

Same pattern as Task 9. Replace all hardcoded colors with token references.

Key additions:
- NoteView: amber/gold accent → `var(--color-accent)`
- CategoriesView: filter active → `var(--color-primary)` based

- [ ] **Step 1: Migrate HomeView.svelte**

Replace hardcoded colors in `<style>`.

- [ ] **Step 2: Migrate NoteView.svelte**

Same pattern. Amber accent → `var(--color-accent)`.

- [ ] **Step 3: Migrate CategoriesView.svelte**

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/views/HomeView.svelte src/lib/components/views/NoteView.svelte src/lib/components/views/CategoriesView.svelte
git commit -m "feat: migrate HomeView, NoteView, CategoriesView to CSS tokens"
```

---

## Task 11: Migrate component styles batch 3 — Toast, Settings, Entry bodies

**Files:**
- Modify: `src/App.svelte` (toast `<style>`)
- Modify: `src/lib/components/views/SettingsView.svelte`
- Modify: `src/lib/components/entry/TextEntryBody.svelte`
- Modify: `src/lib/components/entry/ImageEntryBody.svelte`
- Modify: `src/lib/components/entry/FileEntryBody.svelte`

- [ ] **Step 1: Migrate toast styles in App.svelte**

In the `<style>` section of App.svelte, replace toast hardcoded colors:
- `rgba(15, 23, 42, 0.92)` → `var(--surface-2)`
- `rgba(125, 211, 252, 0.3)` → `color-mix(in srgb, var(--color-primary) 30%, transparent)`
- `rgba(125, 211, 252, 0.9)` → `var(--color-primary)`
- `rgba(248, 113, 113, ...)` → `var(--color-danger)` based

- [ ] **Step 2: Migrate TextEntryBody.svelte, ImageEntryBody.svelte, FileEntryBody.svelte**

Replace all hardcoded colors with token refs.

- [ ] **Step 3: Partially migrate SettingsView.svelte**

Only migrate the style tokens for the CURRENT SettingsView layout (do NOT redesign yet — that's Task 13). Replace hardcoded colors so the existing settings page renders correctly with the token system. The full redesign comes later.

- [ ] **Step 4: Verify full app renders**

```bash
cd E:/codex-prj/Scratchpad && pnpm check && pnpm tauri dev
```

Visually confirm: dark-glass theme looks identical to before (tokens produce same colors).

- [ ] **Step 5: Commit**

```bash
git add src/App.svelte src/lib/components/views/SettingsView.svelte src/lib/components/entry/TextEntryBody.svelte src/lib/components/entry/ImageEntryBody.svelte src/lib/components/entry/FileEntryBody.svelte
git commit -m "feat: migrate toast, settings, entry bodies to CSS tokens"
```

---

## Task 12: Debounce save logic in App.svelte

**Files:**
- Modify: `src/App.svelte`

System theme detection with proper lifecycle was already set up in Task 8 (synchronous `onMount` + `systemDark` `$state`). This task adds debounce logic to `updatePreferences` so that appearance changes are debounced (300ms) while system settings (autostart) save immediately.

- [ ] **Step 1: Replace `updatePreferences` in App.svelte**

Replace the existing `updatePreferences` function (around line 251-270) with:

```typescript
let saveTimer: ReturnType<typeof setTimeout> | null = null

async function updatePreferences(next: DockPreferences) {
  const prev = preferences
  preferences = next  // immediate visual effect via $effect

  // Detect if system settings changed (need immediate save + OS sync)
  const autostartChanged = prev?.launchOnStartup !== next.launchOnStartup

  if (saveTimer) { clearTimeout(saveTimer); saveTimer = null }

  if (autostartChanged) {
    // Immediate save for system settings
    try {
      await dockApi.setPreferences(next)
      // Sync autostart with OS
      try {
        const { enable, disable, isEnabled } = await import('@tauri-apps/plugin-autostart')
        if (next.launchOnStartup) {
          if (!(await isEnabled())) await enable()
        } else {
          if (await isEnabled()) await disable()
        }
      } catch {
        // autostart plugin may not be available in dev
      }
    } catch (e) {
      showToast(`保存失败: ${formatError(e)}`, 'error')
    }
  } else {
    // Debounce appearance/theme changes (300ms)
    saveTimer = setTimeout(async () => {
      try {
        await dockApi.setPreferences(preferences!)
      } catch (e) {
        showToast(`保存失败: ${formatError(e)}`, 'error')
      }
      saveTimer = null
    }, 300)
  }
}
```

Note: `preferences` is always set immediately so the `$effect` from Task 8 applies CSS tokens without delay. The debounce only affects the DB write, not the visual preview.

- [ ] **Step 2: Test system theme switching + debounce**

Run `pnpm tauri dev`, test:
- Switch between preset themes: visual changes immediate, DB writes debounced
- Toggle autostart: immediate save (not debounced)
- Drag sliders rapidly: last value saved, no excess writes

- [ ] **Step 3: Commit**

```bash
git add src/App.svelte
git commit -m "feat: add debounced preference save with immediate system settings"
```

---

## Task 13: Settings page redesign — theme + appearance sections

**Files:**
- Rewrite: `src/lib/components/views/SettingsView.svelte`

This is the largest single task. The SettingsView is completely rewritten with:
- Collapsible sections
- Theme preset cards
- Appearance controls (sliders, color pickers, spacing/radius selectors)

**Save architecture:** SettingsView does NOT save directly to DB. It calls `onChange(next)` (parent `updatePreferences` in App.svelte), which handles immediate visual preview and debounced DB write. SettingsView is a pure UI layer.

**Sub-components:** All implemented as **inline DOM** within SettingsView.svelte (no separate files). Each is a simple inline helper pattern — a wrapper `<div>` with CSS classes, not a Svelte component. This avoids file bloat for UI elements specific to this one page. The inline elements are:

- **Collapsible section** — `<details>/<summary>` or a `<div>` with `class="section"`, `class="section-header"` (click toggles body), `class="section-body"`
- **Slider row** — `<div class="row"><span class="label">...</span><input type="range"/><span class="value">...</span></div>`
- **Color row** — `<div class="row"><span class="label">...</span><input type="color"/><input type="range" min="0" max="100"/> (alpha)</div>`
- **Segment row** — `<div class="row"><span class="label">...</span><div class="segments">{#each ...}<button>...</button>{/each}</div></div>`
- **Toggle row** — `<div class="row"><span class="label">...</span><div class="toggle" onclick=...></div></div>`
- **Font picker row** — reuses the existing font search dropdown pattern from the current SettingsView
- **Proxy row** — `<div class="row"><input placeholder="IP地址"/><span>:</span><input placeholder="端口"/><button>保存</button><button>清除</button></div>`
- **Expert token editor** — `{#each Object.entries(TOKEN_SCHEMA)}` with input + validation

- [ ] **Step 1: Implement the new SettingsView**

Key implementation points:

**Props (unchanged from current):**
```typescript
let { preferences, onChange, onBack } = $props<{
  preferences: DockPreferences
  onChange: (next: DockPreferences) => void
  onBack: () => void
}>()
```

**Local state derived from preferences:**
```typescript
let themeAuto = $state(preferences.themeMode === 'system')
let expertMode = $state(false)

// Local state for appearance fields — initialized from preferences
// and synced back via onChange when changed
```

**Theme state transitions:**

```typescript
function selectPreset(id: string) {
  themeAuto = false
  onChange({
    ...preferences,
    themeMode: 'preset',
    themePresetId: id,
    customBasePresetId: '',
    themeOverrides: {},
  })
}

function toggleThemeAuto() {
  themeAuto = !themeAuto
  onChange({
    ...preferences,
    themeMode: themeAuto ? 'system' : 'preset',
  })
}

// When any micro-adjustment slider/color/segment changes:
function onAppearanceChange(overrides: Record<string, string>) {
  onChange({
    ...preferences,
    themeMode: 'custom',
    customBasePresetId: preferences.themePresetId || 'dark-glass',
    themeOverrides: overrides,
  })
}
```

**Proxy handling:**
Proxy has its own local state (`proxyIp`, `proxyPort`) separate from `preferences.updateProxy`. The "保存" button calls `onChange({ ...preferences, updateProxy: \`${proxyIp}:${proxyPort}\` })`. The "清除" button calls `onChange({ ...preferences, updateProxy: '' })`. Since `launchOnStartup` is unchanged, the debounce applies — but since proxy changes are explicit button clicks (not continuous slider drags), the 300ms debounce is appropriate.

**Template structure:**
```svelte
<div class="settings-view">
  <!-- Theme section -->
  <div class="section">
    <div class="section-header" onclick={() => themeOpen = !themeOpen}>
      <span>主题</span><span class="chevron">▾</span>
    </div>
    {#if themeOpen}
    <div class="section-body">
      <!-- 跟随系统 toggle -->
      <div class="row">
        <span class="label">跟随系统</span>
        <div class="toggle" class:active={themeAuto} onclick={toggleThemeAuto}>...</div>
      </div>
      {#if !themeAuto}
        <!-- 3 theme preset cards -->
        <div class="theme-cards">
          {#each Object.values(THEME_PRESETS) as preset}
            <button class="theme-card" class:active={preferences.themePresetId === preset.id}
                    onclick={() => selectPreset(preset.id)}>
              <div class="swatch" style="background:{preset.tokens['--surface-0']}"></div>
              <span>{preset.name}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
    {/if}
  </div>

  <!-- Appearance section -->
  <div class="section">
    <div class="section-header" onclick={() => appearOpen = !appearOpen}>
      <span>外观</span><span class="chevron">▾</span>
    </div>
    {#if appearOpen}
    <div class="section-body">
      <!-- Slider rows for bg opacity, card bg color+alpha, accent color+alpha, text color+alpha -->
      <!-- Segment rows for spacing (紧凑/标准/宽松), radius (锐利/微圆/圆润) -->
      <!-- Slider rows for uiTextSizePx (10-16), contentTextSizePx (12-20) -->
    </div>
    {/if}
  </div>

  <!-- Font section (collapsed by default) -->
  <!-- Update section: proxy IP+port + check update -->
  <!-- Advanced section: autostart toggle, expert mode toggle, reset buttons -->
</div>
```

- [ ] **Step 2: Verify in browser**

```bash
cd E:/codex-prj/Scratchpad && pnpm tauri dev
```

Test: switch between 3 themes, drag sliders, verify immediate preview + debounced save.

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/views/SettingsView.svelte
git commit -m "feat: redesign settings page with theme selection and collapsible sections"
```

---

## Task 14: Token validation + expert mode

**Files:**
- Create: `src/lib/themes/token-schema.ts`
- Modify: `src/lib/components/views/SettingsView.svelte` (expert token editor section)

- [ ] **Step 1: Create token-schema.ts**

```typescript
export type TokenType = 'color' | 'length' | 'shadow'

export interface TokenSchemaEntry {
  type: TokenType
  label: string
  min?: number
  max?: number
}

export const TOKEN_SCHEMA: Record<string, TokenSchemaEntry> = {
  // Color tokens (17)
  '--color-primary':        { type: 'color', label: '主强调色' },
  '--color-primary-light':  { type: 'color', label: '强调色浅' },
  '--color-primary-faint':  { type: 'color', label: '强调色极浅' },
  '--color-accent':         { type: 'color', label: '次强调色' },
  '--color-danger':         { type: 'color', label: '危险色' },
  '--color-success':        { type: 'color', label: '成功色' },
  '--color-info':           { type: 'color', label: '信息色' },
  '--color-file':           { type: 'color', label: '文件色' },
  '--surface-0':            { type: 'color', label: '容器底色' },
  '--surface-1':            { type: 'color', label: '卡片表面' },
  '--surface-2':            { type: 'color', label: '凹陷表面' },
  '--text-primary':         { type: 'color', label: '主文字' },
  '--text-muted':           { type: 'color', label: '弱文字' },
  '--text-faint':           { type: 'color', label: '极淡文字' },
  '--border-default':       { type: 'color', label: '默认边框' },
  '--border-subtle':        { type: 'color', label: '分割线' },
  '--border-emphasis':      { type: 'color', label: '强调边框' },
  // Effect token (1)
  '--shadow-default':       { type: 'shadow', label: '基础阴影' },
  // Layout tokens (6)
  '--space-sm':             { type: 'length', label: '间距-小', min: 0.05, max: 1.0 },
  '--space-md':             { type: 'length', label: '间距-中', min: 0.05, max: 1.0 },
  '--space-lg':             { type: 'length', label: '间距-大', min: 0.05, max: 2.0 },
  '--radius-sm':            { type: 'length', label: '圆角-小', min: 0, max: 2.0 },
  '--radius-md':            { type: 'length', label: '圆角-中', min: 0, max: 2.0 },
  '--radius-lg':            { type: 'length', label: '圆角-大', min: 0, max: 2.0 },
}

export function validateToken(key: string, value: string): { valid: boolean; error?: string } {
  const schema = TOKEN_SCHEMA[key]
  if (!schema) return { valid: false, error: '未知 token' }

  switch (schema.type) {
    case 'color': {
      // Accept rgba(), #hex, oklch()
      if (/^(rgba?\(|#|oklch\()/.test(value.trim())) return { valid: true }
      return { valid: false, error: '格式应为 rgba(...), #hex, 或 oklch(...)' }
    }
    case 'length': {
      const num = parseFloat(value)
      if (isNaN(num)) return { valid: false, error: '请输入数值' }
      if (schema.min !== undefined && num < schema.min) return { valid: false, error: `最小 ${schema.min}` }
      if (schema.max !== undefined && num > schema.max) return { valid: false, error: `最大 ${schema.max}` }
      return { valid: true }
    }
    case 'shadow': {
      // Basic box-shadow format check
      if (/\d/.test(value)) return { valid: true }
      return { valid: false, error: '格式应为 box-shadow 值' }
    }
  }
}
```

- [ ] **Step 2: Wire validation into expert token editor in SettingsView**

Each token input should call `validateToken()` on blur. If invalid, show red border and error message, don't write to overrides.

- [ ] **Step 3: Commit**

```bash
git add src/lib/themes/token-schema.ts src/lib/components/views/SettingsView.svelte
git commit -m "feat: add token validation schema and expert mode validation"
```

---

## Task 15: Final integration test + cleanup

**Files:**
- Modify: `src/App.svelte` (remove old field references if any remain)
- Delete: `src/main.js`
- Delete: `src/main.js.map`

- [ ] **Step 1: Delete stale compiled artifacts**

```bash
rm -f src/main.js src/main.js.map
```

- [ ] **Step 2: Verify tsconfig doesn't output to src/**

Check `tsconfig.json` that `outDir` is not `src/`.

- [ ] **Step 3: Run full verification**

```bash
cd E:/codex-prj/Scratchpad && pnpm check
cd src-tauri && cargo test
cd .. && pnpm test:unit
```

Expected: ALL PASS

- [ ] **Step 4: Run `pnpm tauri dev` and manually verify**

Checklist:
- [ ] Dark-glass theme renders identically to pre-migration
- [ ] Switch to light-matte: all components render with correct colors, no hardcoded dark remnants
- [ ] Switch to light-frosted: warm tones, glass effect preserved
- [ ] Toggle "跟随系统" on/off: correctly follows/locks
- [ ] Drag background opacity slider: instant preview, no flicker
- [ ] Drag font size sliders: both independent
- [ ] Expert mode: edit a color, verify it persists after app restart
- [ ] Expert mode: enter invalid value, verify red border and rejection
- [ ] Reset "当前主题": returns to preset defaults
- [ ] Reset "全部设置": resets everything including proxy
- [ ] Toast messages visible on both dark and light themes
- [ ] Entry cards (text/image/file) render correctly on all themes
- [ ] Drag reorder indicator visible on light themes
- [ ] Proxy save/clear works

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: cleanup stale artifacts and verify full integration"
```

---

## Post-implementation: Backlog items

After the theme system is complete, address these as separate follow-up PRs:

1. Search clear button `x` → `×` character
2. Action button click targets 1.6rem → 1.8rem
3. Card hover animation enhancement
4. View switch fade/slide transitions
5. Empty state SVG illustrations
6. Scrollbar width optimization (3px → 4-5px or hover-expand)
