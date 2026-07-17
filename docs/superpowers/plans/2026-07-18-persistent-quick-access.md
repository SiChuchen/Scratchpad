# Persistent Quick Access Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an always-visible main-window Quick Access floating button, promote Record/Search to primary navigation, synchronize the Quick Access theme live with the main window, and tune the Quick Access window to a compact DPI-aware size.

**Architecture:** A presentation-only `QuickAccessFab.svelte` lives in the main `App.svelte` shell while `App.svelte` owns the Tauri invocation and toast handling. A typed preference-sync helper broadcasts optimistic `DockPreferences` changes across webview windows; `QuickAccessApp.svelte` consumes the same preferences and theme engine as the main window. Rust centers and sizes the separate window after converting approved logical dimensions through its active DPI scale factor.

**Tech Stack:** Svelte 5 runes, TypeScript, Tauri 2 event/window APIs, Vitest + Testing Library, Rust, Windows DPI-aware geometry.

---

## File Structure

**Create:**

- `src/lib/state/preferences-sync.ts` — typed cross-window preference event helpers.
- `src/lib/state/preferences-sync.test.ts` — event payload tests.
- `src/lib/components/QuickAccessFab.svelte` — circular presentation component.
- `src/lib/components/QuickAccessFab.test.ts` — interaction and disabled-state tests.

**Modify:**

- `src/App.svelte` — broadcast preferences, invoke Quick Access, mount the global button.
- `src/QuickAccessApp.svelte` and `src/QuickAccessApp.test.ts` — live theme sync and primary mode switch.
- `src/lib/components/views/VaultView.svelte` and its test — remove the duplicate Library action.
- `src/lib/i18n/types.ts`, both locale files, and i18n tests — localized open failure.
- `src-tauri/src/lib.rs` — makes the UI command focus rather than toggle Quick Access, then passes DPI scale to geometry.
- `src-tauri/src/system/window.rs` and `src-tauri/tauri.conf.json` — DPI-aware target/minimum geometry.

No new IPC command or database field is required.

---

### Task 1: Live Cross-Window Theme Synchronization

**Files:**
- Create: `src/lib/state/preferences-sync.ts`
- Create: `src/lib/state/preferences-sync.test.ts`
- Modify: `src/App.svelte:2-15,326-375`
- Modify: `src/QuickAccessApp.svelte:12-23,73-163`
- Modify: `src/QuickAccessApp.test.ts:6-168`

- [ ] **Step 1: Write the failing preference event tests**

Create `src/lib/state/preferences-sync.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { DockPreferences } from '$lib/types/dock'

const mocks = vi.hoisted(() => ({ emit: vi.fn(), listen: vi.fn() }))

vi.mock('@tauri-apps/api/event', () => ({
  emit: mocks.emit,
  listen: mocks.listen,
}))

import {
  PREFERENCES_PREVIEW_EVENT,
  broadcastPreferences,
  listenForPreferenceChanges,
} from './preferences-sync'

const prefs = { themePresetId: 'light-matte' } as DockPreferences

beforeEach(() => {
  vi.clearAllMocks()
  mocks.emit.mockResolvedValue(undefined)
  mocks.listen.mockResolvedValue(vi.fn())
})

describe('preferences sync', () => {
  it('broadcasts the complete preference snapshot', async () => {
    await broadcastPreferences(prefs)
    expect(mocks.emit).toHaveBeenCalledWith(PREFERENCES_PREVIEW_EVENT, prefs)
  })

  it('unwraps the Tauri event payload for subscribers', async () => {
    const onChange = vi.fn()
    await listenForPreferenceChanges(onChange)
    mocks.listen.mock.calls[0][1]({ payload: prefs })
    expect(onChange).toHaveBeenCalledWith(prefs)
  })
})
```

- [ ] **Step 2: Prove the tests fail before implementation**

Run `pnpm test:unit -- src/lib/state/preferences-sync.test.ts`.

Expected: FAIL because `./preferences-sync` does not exist.

- [ ] **Step 3: Implement the typed event helper**

Create `src/lib/state/preferences-sync.ts`:

```ts
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { DockPreferences } from '$lib/types/dock'

export const PREFERENCES_PREVIEW_EVENT = 'dock-preferences-preview'

export function broadcastPreferences(prefs: DockPreferences): Promise<void> {
  return emit(PREFERENCES_PREVIEW_EVENT, prefs)
}

export function listenForPreferenceChanges(
  onChange: (prefs: DockPreferences) => void,
): Promise<UnlistenFn> {
  return listen<DockPreferences>(PREFERENCES_PREVIEW_EVENT, (event) => {
    onChange(event.payload)
  })
}
```

- [ ] **Step 4: Verify the helper passes**

Run `pnpm test:unit -- src/lib/state/preferences-sync.test.ts`.

Expected: 2 tests PASS.

- [ ] **Step 5: Add a failing Quick Access live-theme test**

Extend the hoisted mock in `src/QuickAccessApp.test.ts`:

```ts
listeners: new Map<string, (event: { payload: unknown }) => void>(),
```

Replace the default listener setup in `beforeEach`:

```ts
mocks.listeners.clear()
mocks.listen.mockImplementation(
  async (event: string, handler: (event: { payload: unknown }) => void) => {
    mocks.listeners.set(event, handler)
    return vi.fn()
  },
)
```

Import `PREFERENCES_PREVIEW_EVENT` and `computeThemeTokens`, then add:

```ts
it('applies live preference changes from the main window', async () => {
  render(QuickAccessApp)
  await screen.findByRole('tab', { name: '记录' })

  const next = preferences()
  next.themePresetId = 'light-matte'
  next.fontFamilyZh = 'SimSun'
  const listener = mocks.listeners.get(PREFERENCES_PREVIEW_EVENT)
  expect(listener).toBeDefined()
  listener?.({ payload: next })

  const expected = computeThemeTokens(next, true)
  await waitFor(() => {
    expect(document.documentElement.style.getPropertyValue('--surface-0')).toBe(
      expected['--surface-0'],
    )
    expect(document.documentElement.style.getPropertyValue('--font-family-zh')).toBe('SimSun')
  })
})
```

- [ ] **Step 6: Prove the live-theme test fails**

Run `pnpm test:unit -- src/QuickAccessApp.test.ts`.

Expected: FAIL because no `dock-preferences-preview` listener is registered.

- [ ] **Step 7: Broadcast optimistic changes from the main window**

Import `broadcastPreferences` in `src/App.svelte`. Immediately after `preferences = next` in `updatePreferences`, add:

```ts
preferences = next
void broadcastPreferences(next).catch(() => {})
```

Leave existing immediate/debounced persistence unchanged so both windows match before SQLite persistence completes.

- [ ] **Step 8: Consume one shared preference snapshot in Quick Access**

Import `listenForPreferenceChanges` in `src/QuickAccessApp.svelte`. Add:

```ts
function applyPreferences(next: DockPreferences) {
  if (next.language && preferences?.language !== next.language) {
    loadLocale(next.language)
  }
  preferences = next
}
```

Use `applyPreferences(prefs)` for initial load and the existing focus-time reload. During `onMount`, register:

```ts
unlisteners.push(
  await listenForPreferenceChanges((next) => {
    applyPreferences(next)
  }),
)
```

Keep the existing `$effect` as the only token/font application path.

- [ ] **Step 9: Run focused synchronization tests**

Run:

```powershell
pnpm test:unit -- src/lib/state/preferences-sync.test.ts src/QuickAccessApp.test.ts src/lib/themes/__tests__/engine.test.ts
```

Expected: all selected tests PASS.

- [ ] **Step 10: Commit the synchronization slice**

```powershell
git add src/App.svelte src/QuickAccessApp.svelte src/QuickAccessApp.test.ts src/lib/state/preferences-sync.ts src/lib/state/preferences-sync.test.ts
git commit -m "sync quick access theme with main window"
```

---

### Task 2: Persistent Floating Quick Access Button

**Files:**
- Create: `src/lib/components/QuickAccessFab.svelte`
- Create: `src/lib/components/QuickAccessFab.test.ts`
- Modify: `src/App.svelte:1-25,397-423,673-735`
- Modify: `src/lib/components/views/VaultView.svelte:18-24,211-224,350-390,570-585`
- Modify: `src/lib/components/views/VaultView.test.ts:106-150`
- Modify: `src/lib/i18n/types.ts:49-79`
- Modify: `src/lib/i18n/locales/zh-CN.ts:50-80`
- Modify: `src/lib/i18n/locales/en.ts:50-80`
- Modify: `src/lib/i18n/__tests__/i18n.test.ts:45-65`
- Modify: `src-tauri/src/lib.rs:367-380`

- [ ] **Step 1: Add failing locale assertions**

Add to `src/lib/i18n/__tests__/i18n.test.ts`:

```ts
expect(zhCN.quickAccess.openFailed).toBe('无法打开快速入口')
expect(en.quickAccess.openFailed).toBe('Could not open quick access')
```

Run `pnpm test:unit -- src/lib/i18n/__tests__/i18n.test.ts`.

Expected: FAIL because `openFailed` is missing.

- [ ] **Step 2: Add the typed messages**

Add `openFailed: string` to `LocaleMessages.quickAccess`. Add:

```ts
// zh-CN.ts
openFailed: '无法打开快速入口',

// en.ts
openFailed: 'Could not open quick access',
```

Run the i18n test again. Expected: PASS.

- [ ] **Step 3: Write failing component tests**

Create `src/lib/components/QuickAccessFab.test.ts`:

```ts
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { loadLocale } from '$lib/i18n'
import QuickAccessFab from './QuickAccessFab.svelte'

afterEach(cleanup)

describe('QuickAccessFab', () => {
  it('exposes and invokes the global action', async () => {
    loadLocale('zh-CN')
    const onOpen = vi.fn()
    render(QuickAccessFab, { onOpen })
    const button = screen.getByRole('button', { name: '打开全局快速入口' })
    await fireEvent.click(button)
    expect(onOpen).toHaveBeenCalledTimes(1)
  })

  it('blocks activation while opening', async () => {
    loadLocale('en')
    const onOpen = vi.fn()
    render(QuickAccessFab, { onOpen, disabled: true })
    const button = screen.getByRole('button', { name: 'Open quick access' })
    expect(button).toBeDisabled()
    await fireEvent.click(button)
    expect(onOpen).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 4: Prove the component test fails**

Run `pnpm test:unit -- src/lib/components/QuickAccessFab.test.ts`.

Expected: FAIL because `QuickAccessFab.svelte` does not exist.

- [ ] **Step 5: Implement the floating component**

Create `src/lib/components/QuickAccessFab.svelte`:

```svelte
<script lang="ts">
  import { messages } from '$lib/i18n'

  interface Props {
    onOpen: () => void | Promise<void>
    disabled?: boolean
  }

  let { onOpen, disabled = false }: Props = $props()
</script>

<button
  type="button"
  class="quick-access-fab"
  onclick={onOpen}
  {disabled}
  aria-label={messages.library.openQuickAccess}
  title={messages.library.openQuickAccess}
>
  <svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
    <path d="M13 2 5 14h6l-1 8 8-12h-6l1-8Z" stroke-linejoin="round" />
  </svg>
</button>

<style>
  .quick-access-fab {
    position: absolute;
    right: 0.8rem;
    bottom: 0.8rem;
    z-index: 90;
    width: 3rem;
    height: 3rem;
    display: grid;
    place-items: center;
    padding: 0;
    border: 1px solid color-mix(in srgb, var(--color-primary) 65%, transparent);
    border-radius: 50%;
    background: color-mix(in srgb, var(--color-primary) 82%, var(--surface-0));
    color: var(--surface-0);
    box-shadow: 0 0.45rem 1.2rem color-mix(in srgb, var(--color-primary) 30%, transparent);
    cursor: pointer;
    transition: transform 0.14s ease, box-shadow 0.14s ease, filter 0.14s ease;
  }

  .quick-access-fab svg { width: 1.25rem; height: 1.25rem; }
  .quick-access-fab:hover:not(:disabled) { transform: translateY(-2px); filter: brightness(1.08); }
  .quick-access-fab:active:not(:disabled) { transform: translateY(0) scale(0.96); }
  .quick-access-fab:focus-visible { outline: 2px solid var(--text-primary); outline-offset: 3px; }
  .quick-access-fab:disabled { cursor: wait; opacity: 0.65; }

  @media (prefers-reduced-motion: reduce) {
    .quick-access-fab { transition: none; }
  }
</style>
```

- [ ] **Step 6: Verify the component tests pass**

Run `pnpm test:unit -- src/lib/components/QuickAccessFab.test.ts`.

Expected: 2 tests PASS.

- [ ] **Step 7: Wire it into the top-level shell**

Import `QuickAccessFab` in `src/App.svelte`, add:

```ts
let quickAccessOpening = $state(false)

async function openQuickAccess() {
  if (quickAccessOpening) return
  quickAccessOpening = true
  try {
    await invoke('ipc_open_quick_access')
  } catch (e) {
    showToast(`${messages.quickAccess.openFailed}: ${formatError(e)}`, 'error')
  } finally {
    quickAccessOpening = false
  }
}
```

Mount it inside `{#key langKey}` but after the conditional view branch:

```svelte
  {/if}
  <QuickAccessFab onOpen={openQuickAccess} disabled={quickAccessOpening} />
{/key}
```

Its `z-index: 90` stays below toast `110`, drag overlay `200`, and confirmation dialog `300`.

- [ ] **Step 8: Make the UI command consistently open and focus**

Replace `ipc_open_quick_access` in `src-tauri/src/lib.rs` with:

```rust
#[tauri::command]
fn ipc_open_quick_access(app: tauri::AppHandle) -> Result<(), String> {
    if app.get_webview_window("quick-access").is_none() {
        return Err("quick-access window not found".to_string());
    }
    show_quick_access_centered(&app);
    Ok(())
}
```

Update its comment to say that UI activation always shows/recenters/focuses. Do not change the separate global-shortcut callbacks: they continue toggling visibility.

- [ ] **Step 9: Add a failing duplicate-action regression test**

In `src/lib/components/views/VaultView.test.ts`, add:

```ts
it('does not render a second Library-only quick access action', async () => {
  render(VaultView, { notify: vi.fn() })
  await waitFor(() => expect(mocks.listEntries).toHaveBeenCalled())
  expect(
    screen.queryByRole('button', { name: messages.library.openQuickAccess }),
  ).not.toBeInTheDocument()
})
```

Expected before cleanup: FAIL because the current header action exists.

- [ ] **Step 10: Remove the duplicate Vault action**

From `VaultView.svelte`, remove the `invoke` import, `openQuickAccess` function, Quick Access button block, and `.quick-access-btn` styles. Keep the New menu alignment unchanged.

- [ ] **Step 11: Run the focused UI tests**

```powershell
pnpm test:unit -- src/lib/components/QuickAccessFab.test.ts src/lib/components/views/VaultView.test.ts src/lib/i18n/__tests__/i18n.test.ts
```

Expected: all selected tests PASS.

- [ ] **Step 12: Commit the persistent-entry slice**

```powershell
git add src/App.svelte src/lib/components/QuickAccessFab.svelte src/lib/components/QuickAccessFab.test.ts src/lib/components/views/VaultView.svelte src/lib/components/views/VaultView.test.ts src/lib/i18n/types.ts src/lib/i18n/locales/zh-CN.ts src/lib/i18n/locales/en.ts src/lib/i18n/__tests__/i18n.test.ts src-tauri/src/lib.rs
git commit -m "add persistent quick access button"
```

---

### Task 3: Promote Record and Search to Primary Navigation

**Files:**
- Modify: `src/QuickAccessApp.svelte:210-242,274-317`
- Modify: `src/QuickAccessApp.test.ts:103-190`

- [ ] **Step 1: Add a failing structural test**

Add to `src/QuickAccessApp.test.ts`:

```ts
it('renders both primary modes as icon-labelled segments', async () => {
  render(QuickAccessApp)
  const record = await screen.findByRole('tab', { name: '记录' })
  const search = screen.getByRole('tab', { name: '搜索' })

  expect(record).toHaveAttribute('data-primary-mode', 'record')
  expect(search).toHaveAttribute('data-primary-mode', 'search')
  expect(record.querySelector('svg[aria-hidden="true"]')).toBeInTheDocument()
  expect(search.querySelector('svg[aria-hidden="true"]')).toBeInTheDocument()
})
```

Run `pnpm test:unit -- src/QuickAccessApp.test.ts`.

Expected: FAIL because the attributes and icons do not exist.

- [ ] **Step 2: Add visible icon-labelled mode markup**

Add `data-primary-mode="record"` to the Record tab and replace its text child with:

```svelte
<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
  <path d="M12 20h9" />
  <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4Z" stroke-linejoin="round" />
</svg>
<span>{messages.quickAccess.record}</span>
```

Add `data-primary-mode="search"` to the Search tab and replace its text child with:

```svelte
<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8">
  <circle cx="11" cy="11" r="7" />
  <path d="m20 20-3.6-3.6" />
</svg>
<span>{messages.quickAccess.search}</span>
```

Do not change IDs, roles, `aria-selected`, `aria-controls`, roving `tabindex`, or `switchMode`.

- [ ] **Step 3: Replace the small-tab CSS with a full-width switch**

Use:

```css
.qa-tablist {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0.4rem;
  padding: 0.45rem;
  border-bottom: 1px solid var(--border-emphasis);
  background: color-mix(in srgb, var(--surface-0) 86%, transparent);
}

.qa-tab {
  appearance: none;
  min-width: 0;
  min-height: 2.9rem;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 0.45rem;
  padding: 0.55rem 0.8rem;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md, 0.4rem);
  background: var(--surface-1);
  color: var(--text-muted);
  font: inherit;
  font-size: var(--font-md, 0.85rem);
  font-weight: 600;
  cursor: pointer;
  transition: color 0.14s, border-color 0.14s, background 0.14s, box-shadow 0.14s;
}

.qa-tab svg {
  width: 1rem;
  height: 1rem;
  flex: 0 0 auto;
}

.qa-tab:hover {
  color: var(--text-primary);
  border-color: var(--border-emphasis);
  background: color-mix(in srgb, var(--color-primary) 9%, var(--surface-1));
}

.qa-tab.active {
  color: var(--color-primary);
  border-color: color-mix(in srgb, var(--color-primary) 55%, transparent);
  background: color-mix(in srgb, var(--color-primary) 16%, var(--surface-1));
  box-shadow: inset 0 -2px 0 var(--color-primary);
}

.qa-tab:focus-visible {
  outline: 2px solid var(--color-primary);
  outline-offset: 2px;
}
```

Add `border-radius: var(--radius-lg, 0.55rem)` to `.quick-shell` so the undecorated window follows the main theme's corner language.

- [ ] **Step 4: Run Quick Access behavior tests**

```powershell
pnpm test:unit -- src/QuickAccessApp.test.ts src/lib/components/quick-access/CaptureMode.test.ts src/lib/components/quick-access/SearchMode.test.ts
```

Expected: all tests PASS, including preserved input and `Ctrl+Tab` focus behavior.

- [ ] **Step 5: Run Svelte static checks**

Run `pnpm check`.

Expected: 0 errors and no new warning in `QuickAccessApp.svelte`.

- [ ] **Step 6: Commit the navigation slice**

```powershell
git add src/QuickAccessApp.svelte src/QuickAccessApp.test.ts
git commit -m "promote quick access mode navigation"
```

---

### Task 4: Tune DPI-Aware Quick Access Window Dimensions

**Files:**
- Modify: `src-tauri/src/system/window.rs:120-154,177-239`
- Modify: `src-tauri/src/lib.rs:99-119`
- Modify: `src-tauri/tauri.conf.json:45-62`

- [ ] **Step 1: Replace geometry expectations with scale-aware tests**

In `src-tauri/src/system/window.rs`, replace the current Quick Access geometry tests with:

```rust
#[test]
fn fit_and_center_quick_access_uses_logical_target_at_100_percent() {
    let work = WorkRect::new(0, 0, 1920, 1080);
    let (x, y, w, h) = fit_and_center_quick_access(work, 1.0);
    assert_eq!((w, h), (680, 480));
    assert_eq!((x, y), ((1920 - 680) / 2, (1080 - 480) / 2));
}

#[test]
fn fit_and_center_quick_access_scales_logical_target() {
    let work = WorkRect::new(0, 0, 2560, 1440);
    assert_eq!(fit_and_center_quick_access(work, 1.25).2, 850);
    assert_eq!(fit_and_center_quick_access(work, 1.25).3, 600);
    assert_eq!(fit_and_center_quick_access(work, 1.5).2, 1020);
    assert_eq!(fit_and_center_quick_access(work, 1.5).3, 720);
}

#[test]
fn fit_and_center_quick_access_clamps_small_work_area() {
    let work = WorkRect::new(0, 0, 800, 500);
    let (x, y, w, h) = fit_and_center_quick_access(work, 1.0);
    assert_eq!((w, h), (680, 450));
    assert_eq!((x, y), ((800 - 680) / 2, (500 - 450) / 2));
}

#[test]
fn fit_and_center_quick_access_handles_negative_monitor_coordinates() {
    let work = WorkRect::new(-1920, 0, 0, 1080);
    let (x, y, w, h) = fit_and_center_quick_access(work, 1.0);
    assert_eq!((w, h), (680, 480));
    assert_eq!((x, y), (-1920 + (1920 - 680) / 2, (1080 - 480) / 2));
}

#[test]
fn runtime_min_size_is_scale_aware_and_clamped() {
    let large = WorkRect::new(0, 0, 2560, 1440);
    assert_eq!(runtime_min_size(&large, 1.0), (480, 340));
    assert_eq!(runtime_min_size(&large, 1.5), (720, 510));

    let small = WorkRect::new(0, 0, 400, 300);
    assert_eq!(runtime_min_size(&small, 1.5), (360, 270));
}
```

- [ ] **Step 2: Prove the Rust tests fail**

From `src-tauri/`, run `cargo test system::window::tests --lib`.

Expected: FAIL because the helpers do not accept a scale factor and still target `760 × 520` / `480 × 320`.

- [ ] **Step 3: Implement logical-to-physical sizing**

Replace the Quick Access geometry helpers with:

```rust
const QUICK_ACCESS_WIDTH_LOGICAL: f64 = 680.0;
const QUICK_ACCESS_HEIGHT_LOGICAL: f64 = 480.0;
const QUICK_ACCESS_MIN_WIDTH_LOGICAL: f64 = 480.0;
const QUICK_ACCESS_MIN_HEIGHT_LOGICAL: f64 = 340.0;

fn logical_pixels(value: f64, scale_factor: f64) -> i32 {
    (value * scale_factor).round() as i32
}

pub fn fit_and_center_quick_access(
    work_area: WorkRect,
    scale_factor: f64,
) -> (i32, i32, i32, i32) {
    let work_width = work_area.right - work_area.left;
    let work_height = work_area.bottom - work_area.top;
    let width = logical_pixels(QUICK_ACCESS_WIDTH_LOGICAL, scale_factor)
        .min(work_width * 9 / 10);
    let height = logical_pixels(QUICK_ACCESS_HEIGHT_LOGICAL, scale_factor)
        .min(work_height * 9 / 10);
    let x = work_area.left + (work_width - width) / 2;
    let y = work_area.top + (work_height - height) / 2;
    (x, y, width, height)
}

pub fn runtime_min_size(work_area: &WorkRect, scale_factor: f64) -> (i32, i32) {
    let work_width = work_area.right - work_area.left;
    let work_height = work_area.bottom - work_area.top;
    let min_width = logical_pixels(QUICK_ACCESS_MIN_WIDTH_LOGICAL, scale_factor)
        .min(work_width * 9 / 10);
    let min_height = logical_pixels(QUICK_ACCESS_MIN_HEIGHT_LOGICAL, scale_factor)
        .min(work_height * 9 / 10);
    (min_width, min_height)
}
```

Update comments: work-area inputs and outputs are physical pixels; target/minimum constants are logical pixels converted by `scale_factor`.

- [ ] **Step 4: Pass the active Tauri scale factor**

In `show_quick_access_centered` in `src-tauri/src/lib.rs`, use:

```rust
let scale_factor = quick.scale_factor().unwrap_or(1.0);
let (x, y, w, h) = system::window::fit_and_center_quick_access(work, scale_factor);
let (min_w, min_h) = system::window::runtime_min_size(&work, scale_factor);
```

The cursor still chooses the monitor through `win_monitor_work_area(cx, cy)`; remove the unused cursor arguments from the pure helper call.

- [ ] **Step 5: Align the static logical config**

Set the `quick-access` entry in `src-tauri/tauri.conf.json` to:

```json
"width": 680,
"height": 480,
"minWidth": 480,
"minHeight": 340,
```

Keep all other window properties unchanged.

- [ ] **Step 6: Run focused and full Rust validation**

From `src-tauri/`:

```powershell
cargo test system::window::tests --lib
cargo test
```

Expected: focused geometry tests and full Rust suite PASS.

- [ ] **Step 7: Commit the sizing slice**

```powershell
git add src-tauri/src/system/window.rs src-tauri/src/lib.rs src-tauri/tauri.conf.json
git commit -m "tune quick access window dimensions"
```

Do not stage the pre-existing line-ending-only change in `src-tauri/Cargo.toml`.

---

### Task 5: Integrated UX Verification and Final Adjustment

**Files:**
- Modify only after a measured failure: `src/App.svelte`, `src/lib/components/QuickAccessFab.svelte`, `src/QuickAccessApp.svelte`, `src-tauri/src/system/window.rs`, `src-tauri/tauri.conf.json`, plus the directly corresponding tests.

- [ ] **Step 1: Run complete automated frontend validation**

```powershell
pnpm test:unit
pnpm check
pnpm build
```

Expected: all Vitest tests PASS, `svelte-check` has 0 errors and no new warning in changed files, and the production build completes.

- [ ] **Step 2: Start the desktop app and inspect the floating entry**

Run `pnpm tauri dev` and visit `home`, `categories`, `note`, `vault`, and `settings`.

Verify:

- The 48 px circular control keeps the same bottom-right inset.
- It does not cover top navigation, toast, confirmation dialog, or primary page controls.
- The last control/card in each scrollable page can scroll above the button's reserved safe area.
- Focus is visible; Enter/Space opens Quick Access; repeated activation while opening is blocked.
- Clicking the floating entry while Quick Access is already visible focuses/recenters it instead of hiding it; the global shortcut still toggles.
- The Library page has no duplicate rectangular Quick Access action.

- [ ] **Step 3: Verify the two-mode mental model at both supported sizes**

At the 680 × 480 target and 480 × 340 minimum:

- Record/Search are the first and strongest controls seen.
- Each equal segment stays at least 44 logical pixels high.
- Selected state is distinguishable by fill, border/accent edge, text, and ARIA state.
- Record textarea, Save action, Search input, results, and detail actions remain reachable without horizontal scrolling.
- Mouse and `Ctrl+Tab` switching preserve both modes' input.

- [ ] **Step 4: Verify live shared themes**

Keep Quick Access visible while selecting Dark Glass, Light Matte, Light Frosted, System theme, and one custom primary-color override in the main Settings page.

For every change, verify without hiding/reopening Quick Access:

- Surfaces, text, borders, primary color, radii, spacing, and fonts update with the main window.
- Text and the inactive mode remain readable.
- The floating button remains legible on light and dark surfaces.

- [ ] **Step 5: Verify geometry and use objective adjustment criteria**

Use Rust tests as deterministic 100%/125%/150% DPI coverage, then inspect the current Windows scale visually:

- Default open centers on the cursor's monitor at the 680 × 480 logical target.
- Resize stops at 480 × 340 logical pixels unless the 90% work-area clamp is smaller.
- Reopening returns to the approved centered target rather than retaining an awkward resize.
- Negative-coordinate monitor centering is covered by the pure geometry test.

Only adjust a dimension if there is clipped content, overlap, horizontal scrolling, an unreachable primary action, or fewer than two useful search results visible at target size. If adjusted, change the logical constant, Tauri config, and exact Rust expectations together, then rerun Task 4 Step 6 and Task 5 Step 1.

- [ ] **Step 6: Review the final diff and workspace**

```powershell
git diff HEAD~4 --check
git diff HEAD~4 --stat
git status --short
```

Confirm generated output is unstaged, `src-tauri/Cargo.toml` remains unstaged, all colors come from shared theme tokens, and no duplicate Vault action remains.

- [ ] **Step 7: Commit only a measured visual adjustment**

If verification required tracked changes, stage only those feature files and tests, then run:

```powershell
git commit -m "polish quick access layout"
```

If no adjustment was needed, do not make an empty commit; record the tested themes and sizes in the final summary.
