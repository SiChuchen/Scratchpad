# Quick Access Persistent Work Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep Quick Access visible across focus changes, add explicit work-panel controls, preserve strict main/Quick Access shortcut toggles, and make search-result information faster to scan and copy.

**Execution status (2026-07-18): Complete.** Automated validation and Windows runtime UX checks passed. Cross-application behavior was validated without exposing stored credential values by combining native focus, shortcut, pin, close, drag, and theme checks with copy/reset regression tests.

**Architecture:** Rust reduces the focus-loss path to sensitive-value reset only and centralizes both visibility toggles so startup and runtime shortcut registration cannot diverge. A presentation-only Svelte window bar delegates native drag, pin, and hide actions to `QuickAccessApp.svelte`. Search details opt into a prominent `VaultEntryDetail`/`CopyableValue` variant that changes hierarchy and interaction density without enlarging the main Library view.

**Tech Stack:** Svelte 5 runes, TypeScript, Tauri 2 window/event APIs, Vitest + Testing Library, Rust unit tests, Windows desktop runtime verification.

---

## File Structure

**Create:**

- `src/lib/components/quick-access/QuickAccessWindowBar.svelte` — deterministic work-panel title bar.
- `src/lib/components/quick-access/QuickAccessWindowBar.test.ts` — accessibility, drag, pin, close, and pending-state tests.

**Modify:**

- `src-tauri/src/system/window.rs` — pure focus-loss and visibility-toggle policies with unit tests.
- `src-tauri/src/lib.rs` — remove blur hiding, centralize strict shortcut toggles, and reuse them in both registration paths.
- `src/QuickAccessApp.svelte` — own pin state and native window actions; render the work-panel bar.
- `src/QuickAccessApp.test.ts` — native-window state and bar integration tests.
- `src/lib/components/vault/CopyableValue.svelte` — prominent text-copy variant and copy-last action order.
- `src/lib/components/vault/CopyableValue.test.ts` — prominent-label and action-order tests.
- `src/lib/components/vault/VaultEntryDetail.svelte` — Quick Access hierarchy and deterministic field ordering.
- `src/lib/components/quick-access/SearchMode.svelte` — opt into the prominent detail and allow vertical detail scrolling.
- `src/lib/components/quick-access/SearchMode.test.ts` — hierarchy, ordering, visible actions, and repeated-copy regressions.
- `src/lib/i18n/types.ts`, `src/lib/i18n/locales/zh-CN.ts`, `src/lib/i18n/locales/en.ts`, and `src/lib/i18n/__tests__/i18n.test.ts` — typed useful-information heading.

No window dimensions, storage schemas, preferences, capture logic, or search ranking code change.

---

### Task 1: Make Focus Loss Persistent and Centralize Shortcut Toggles

**Files:**
- Modify: `src-tauri/src/system/window.rs:120-225`
- Modify: `src-tauri/src/lib.rs:99-120,425-467,846-853,895-940`

- [x] **Step 1: Add failing pure policy tests**

Append to `src-tauri/src/system/window.rs` tests:

```rust
#[test]
fn quick_access_focus_loss_resets_sensitive_values_only() {
    assert!(should_reset_quick_access_on_focus_loss("quick-access", false));
    assert!(!should_reset_quick_access_on_focus_loss("quick-access", true));
    assert!(!should_reset_quick_access_on_focus_loss("main", false));
}

#[test]
fn visible_windows_hide_and_hidden_windows_show() {
    assert_eq!(visibility_toggle_action(true), VisibilityToggleAction::Hide);
    assert_eq!(visibility_toggle_action(false), VisibilityToggleAction::Show);
}
```

- [x] **Step 2: Prove the policy tests fail**

Run from `src-tauri/`:

```powershell
cargo test system::window::tests --lib
```

Expected: compilation fails because `should_reset_quick_access_on_focus_loss`, `VisibilityToggleAction`, and `visibility_toggle_action` are undefined.

- [x] **Step 3: Implement the pure policies**

Add above the existing Quick Access geometry helpers in `src-tauri/src/system/window.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityToggleAction {
    Show,
    Hide,
}

pub fn visibility_toggle_action(is_visible: bool) -> VisibilityToggleAction {
    if is_visible {
        VisibilityToggleAction::Hide
    } else {
        VisibilityToggleAction::Show
    }
}

pub fn should_reset_quick_access_on_focus_loss(label: &str, focused: bool) -> bool {
    label == "quick-access" && !focused
}
```

- [x] **Step 4: Centralize the native toggle operations**

In `src-tauri/src/lib.rs`, import/use `system::window::VisibilityToggleAction` and add after `show_quick_access_centered`:

```rust
fn toggle_main_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    match system::window::visibility_toggle_action(window.is_visible().unwrap_or(false)) {
        system::window::VisibilityToggleAction::Hide => {
            let _ = window.hide();
        }
        system::window::VisibilityToggleAction::Show => {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

fn toggle_quick_access_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("quick-access") else {
        toggle_main_window(app);
        return;
    };
    match system::window::visibility_toggle_action(window.is_visible().unwrap_or(false)) {
        system::window::VisibilityToggleAction::Hide => {
            let _ = window.hide();
        }
        system::window::VisibilityToggleAction::Show => show_quick_access_centered(app),
    }
}
```

In the runtime-update main callback call `toggle_main_window(&app_handle)`, and in its Quick Access callback call `toggle_quick_access_window(app)`. Make the same replacements in startup registration: `toggle_main_window(&app_handle)` for main and `toggle_quick_access_window(app)` for Quick Access. Preserve the existing `ShortcutState::Pressed` guards.

- [x] **Step 5: Remove click-away hiding but preserve masking**

Replace the Tauri window event handler with:

```rust
.on_window_event(|window, event| {
    let focused = !matches!(event, tauri::WindowEvent::Focused(false));
    if system::window::should_reset_quick_access_on_focus_loss(window.label(), focused) {
        let _ = window.emit("vault-sensitive-reset", ());
    }
})
```

There must be no `window.hide()` in this focus-loss path.

- [x] **Step 6: Run focused and full Rust validation**

Run from `src-tauri/`:

```powershell
cargo test system::window::tests --lib
cargo test
```

Expected: all window-policy tests and the full Rust suite pass.

- [x] **Step 7: Commit the native behavior slice**

```powershell
git add src-tauri/src/system/window.rs src-tauri/src/lib.rs
git commit -m "keep quick access visible on focus loss"
```

Do not stage the pre-existing `src-tauri/Cargo.toml` line-ending change.

---

### Task 2: Add the Draggable Pin/Close Work Panel Bar

**Files:**
- Create: `src/lib/components/quick-access/QuickAccessWindowBar.svelte`
- Create: `src/lib/components/quick-access/QuickAccessWindowBar.test.ts`
- Modify: `src/QuickAccessApp.svelte:12-24,31-70,93-152,220-225,290-310`
- Modify: `src/QuickAccessApp.test.ts:6-40,72-170`

- [x] **Step 1: Write failing work-panel component tests**

Create `src/lib/components/quick-access/QuickAccessWindowBar.test.ts`:

```ts
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { loadLocale } from '$lib/i18n'
import QuickAccessWindowBar from './QuickAccessWindowBar.svelte'

afterEach(cleanup)

describe('QuickAccessWindowBar', () => {
  it('exposes pin and close actions', async () => {
    loadLocale('zh-CN')
    const onTogglePin = vi.fn()
    const onHide = vi.fn()
    render(QuickAccessWindowBar, {
      pinned: true,
      onTogglePin,
      onHide,
      onDrag: vi.fn(),
    })

    await fireEvent.click(screen.getByRole('button', { name: '取消置顶' }))
    await fireEvent.click(screen.getByRole('button', { name: '关闭' }))
    expect(onTogglePin).toHaveBeenCalledTimes(1)
    expect(onHide).toHaveBeenCalledTimes(1)
  })

  it('starts dragging only from non-button title space', async () => {
    const onDrag = vi.fn()
    render(QuickAccessWindowBar, {
      pinned: false,
      onTogglePin: vi.fn(),
      onHide: vi.fn(),
      onDrag,
    })

    await fireEvent.mouseDown(screen.getByTestId('quick-access-drag-region'))
    expect(onDrag).toHaveBeenCalledTimes(1)
    await fireEvent.mouseDown(screen.getByRole('button', { name: '置顶' }))
    expect(onDrag).toHaveBeenCalledTimes(1)
  })

  it('blocks repeated pin activation while pending', async () => {
    const onTogglePin = vi.fn()
    render(QuickAccessWindowBar, {
      pinned: true,
      pinPending: true,
      onTogglePin,
      onHide: vi.fn(),
      onDrag: vi.fn(),
    })
    const pin = screen.getByRole('button', { name: '取消置顶' })
    expect(pin).toBeDisabled()
    await fireEvent.click(pin)
    expect(onTogglePin).not.toHaveBeenCalled()
  })
})
```

- [x] **Step 2: Prove the component tests fail**

```powershell
pnpm test:unit -- src/lib/components/quick-access/QuickAccessWindowBar.test.ts
```

Expected: FAIL because `QuickAccessWindowBar.svelte` does not exist.

- [x] **Step 3: Implement the presentation component**

Create `QuickAccessWindowBar.svelte` with these exact props and interaction boundary:

```svelte
<script lang="ts">
  import { messages } from '$lib/i18n'

  interface Props {
    pinned: boolean
    pinPending?: boolean
    onTogglePin: () => void | Promise<void>
    onHide: () => void | Promise<void>
    onDrag: () => void | Promise<void>
  }

  let { pinned, pinPending = false, onTogglePin, onHide, onDrag }: Props = $props()

  function handleMouseDown(event: MouseEvent) {
    if ((event.target as HTMLElement).closest('button')) return
    event.preventDefault()
    void onDrag()
  }
</script>

<div
  class="window-bar"
  data-testid="quick-access-drag-region"
  onmousedown={handleMouseDown}
>
  <span class="accent" aria-hidden="true"></span>
  <span class="title">{messages.library.quickAccess}</span>
  <span class="spacer"></span>
  <button
    type="button"
    class="bar-button pin"
    class:active={pinned}
    disabled={pinPending}
    aria-label={pinned ? messages.nav.unpin : messages.nav.pin}
    title={pinned ? messages.nav.unpin : messages.nav.pin}
    onclick={() => onTogglePin()}
  ><!-- pin SVG --></button>
  <button
    type="button"
    class="bar-button close"
    aria-label={messages.quickAccess.close}
    title={messages.quickAccess.close}
    onclick={() => onHide()}
  ><!-- close SVG --></button>
</div>
```

Use shared theme tokens. Set `.window-bar` to approximately `2.1rem` high, each `.bar-button` to at least `1.9rem` square, make the non-button area draggable, and add clear hover/focus styles. The close hover may use `--color-danger`; no hard-coded palette is allowed.

- [x] **Step 4: Extend the Quick Access window mock and add failing integration tests**

In `src/QuickAccessApp.test.ts`, add hoisted mocks:

```ts
isAlwaysOnTop: vi.fn(),
setAlwaysOnTop: vi.fn(),
startDragging: vi.fn(),
```

Return them from `getCurrentWindow()`, initialize them in `beforeEach`, and add:

```ts
it('reads and toggles the actual Quick Access pin state', async () => {
  mocks.isAlwaysOnTop.mockResolvedValue(true)
  render(QuickAccessApp)
  const pin = await screen.findByRole('button', { name: '取消置顶' })
  await fireEvent.click(pin)
  await waitFor(() => expect(mocks.setAlwaysOnTop).toHaveBeenCalledWith(false))
  expect(screen.getByRole('button', { name: '置顶' })).toBeInTheDocument()
})

it('hides and drags only through explicit work-panel actions', async () => {
  render(QuickAccessApp)
  await fireEvent.mouseDown(await screen.findByTestId('quick-access-drag-region'))
  expect(mocks.startDragging).toHaveBeenCalledTimes(1)
  await fireEvent.click(screen.getByRole('button', { name: '关闭' }))
  expect(mocks.hide).toHaveBeenCalledTimes(1)
})
```

Run `pnpm test:unit -- src/QuickAccessApp.test.ts`; expected FAIL because the bar and pin flow are not wired.

- [x] **Step 5: Wire native window state into `QuickAccessApp.svelte`**

Add state and actions:

```ts
let pinned = $state(true)
let pinPending = $state(false)

async function togglePinned() {
  if (pinPending) return
  pinPending = true
  const next = !pinned
  try {
    await win.setAlwaysOnTop(next)
    pinned = next
  } catch {
    notify(messages.toast.operationFailed, 'error')
  } finally {
    pinPending = false
  }
}

function startDragging() {
  return win.startDragging().catch((e: unknown) => console.error('drag failed:', e))
}
```

During `onMount`, read the actual state without changing it:

```ts
win.isAlwaysOnTop().then((value) => {
  pinned = value
}).catch(() => {})
```

Render before `.qa-tablist`:

```svelte
<QuickAccessWindowBar
  {pinned}
  {pinPending}
  onTogglePin={togglePinned}
  onHide={requestHide}
  onDrag={startDragging}
/>
```

- [x] **Step 6: Run focused work-panel tests and static checks**

```powershell
pnpm test:unit -- src/lib/components/quick-access/QuickAccessWindowBar.test.ts src/QuickAccessApp.test.ts
pnpm check
```

Expected: selected tests pass and `svelte-check` reports zero errors with no new warning in the changed files.

- [x] **Step 7: Commit the work-panel slice**

```powershell
git add src/QuickAccessApp.svelte src/QuickAccessApp.test.ts src/lib/components/quick-access/QuickAccessWindowBar.svelte src/lib/components/quick-access/QuickAccessWindowBar.test.ts
git commit -m "add quick access work panel controls"
```

---

### Task 3: Reorder Search Details and Promote Copy Actions

**Files:**
- Modify: `src/lib/components/vault/CopyableValue.svelte`
- Modify: `src/lib/components/vault/CopyableValue.test.ts`
- Modify: `src/lib/components/vault/VaultEntryDetail.svelte`
- Modify: `src/lib/components/quick-access/SearchMode.svelte`
- Modify: `src/lib/components/quick-access/SearchMode.test.ts`
- Modify: `src/lib/i18n/types.ts`
- Modify: `src/lib/i18n/locales/zh-CN.ts`
- Modify: `src/lib/i18n/locales/en.ts`
- Modify: `src/lib/i18n/__tests__/i18n.test.ts`

- [x] **Step 1: Add the typed useful-information heading with failing assertions**

Add to the existing i18n regression test:

```ts
expect(zhCN.quickAccess.usefulInformation).toBe('可直接使用的信息')
expect(en.quickAccess.usefulInformation).toBe('Information to use')
```

Run `pnpm test:unit -- src/lib/i18n/__tests__/i18n.test.ts`; expected FAIL because the key is missing.

Add `usefulInformation: string` to `LocaleMessages.quickAccess` and the exact Chinese/English messages above, then rerun the test and expect PASS.

- [x] **Step 2: Add failing prominent-copy behavior tests**

Append to `CopyableValue.test.ts`:

```ts
it('shows a larger visible copy action in prominent mode', () => {
  render(CopyableValue, {
    label: '账号',
    value: 'alice',
    prominent: true,
    onCopy: vi.fn(),
  })
  const copy = screen.getByRole('button', { name: '复制 账号' })
  expect(copy).toHaveTextContent('复制')
  expect(copy).toHaveAttribute('data-prominent-action', 'copy')
})

it('keeps copy as the final action for sensitive values', () => {
  render(CopyableValue, {
    label: '密码',
    value: 'secret',
    sensitive: true,
    prominent: true,
    onCopy: vi.fn(),
  })
  const actions = screen.getByTestId('copyable-actions')
  expect(actions.lastElementChild).toBe(
    screen.getByRole('button', { name: '复制 密码' }),
  )
})
```

Run `pnpm test:unit -- src/lib/components/vault/CopyableValue.test.ts`; expected FAIL because `prominent` and the structural attributes do not exist.

- [x] **Step 3: Implement the prominent CopyableValue variant**

Add `prominent?: boolean` to the props with a default of `false`. Apply `class:prominent` to `.copyable-row` and `data-testid="copyable-actions"` to `.actions`.

Render the sensitive reveal button before the copy button. For the copy button:

```svelte
<button
  type="button"
  class="icon-btn copy-btn"
  class:prominent-action={prominent}
  data-prominent-action={prominent ? 'copy' : undefined}
  onclick={handleCopy}
  aria-label={copyAriaLabel}
  title={copyAriaLabel}
>
  {#if prominent}
    <span>{messages.entry.copy}</span>
  {:else}
    <!-- retain the current copy SVG -->
  {/if}
</button>
```

Prominent CSS requirements:

```css
.copyable-row.prominent { min-height: 2.5rem; gap: 0.5rem; }
.copyable-row.prominent .label { width: 4.25rem; font-size: var(--font-sm, 12px); }
.copyable-row.prominent .value { font-size: var(--font-md, 14px); }
.copyable-row.prominent .icon-btn { width: 2rem; height: 2rem; padding: 0; }
.copyable-row.prominent .copy-btn { width: auto; min-width: 3.25rem; padding: 0 0.65rem; font-size: var(--font-sm, 12px); font-weight: 650; }
```

Keep theme-token borders, surfaces, text, hover, focus, and reduced-motion behavior.

- [x] **Step 4: Add failing Quick Access detail hierarchy tests**

Extend the existing SearchMode detail fixture with deliberately out-of-order fields:

```ts
fields: [
  { id: 'f-url', entryId: 'a', key: '网址', value: 'example.test', isSensitive: false, sortOrder: 2 },
  { id: 'f-password', entryId: 'a', key: '密码', value: 'secret', isSensitive: true, sortOrder: 1 },
  { id: 'f-account', entryId: 'a', key: '账号', value: 'alice', isSensitive: false, sortOrder: 0 },
],
```

Add a test that waits for the detail, obtains elements for `标题`, `账号`, `密码`, `网址`, `备注`, and `手动标签`, and asserts their `compareDocumentPosition` order is title -> account -> password -> URL -> notes -> tag. Also assert all four primary copy buttons have visible `复制` text and the password copy button is the last element in its action group.

Run:

```powershell
pnpm test:unit -- src/lib/components/quick-access/SearchMode.test.ts
```

Expected: FAIL because notes/tags currently precede fields and copy actions are compact icons.

- [x] **Step 5: Implement Quick Access hierarchy without changing Library density**

Add `prominent?: boolean` to `VaultEntryDetail.svelte`. Derive a stable copy of fields:

```ts
const sortedFields = $derived(
  [...detail.fields].sort((a, b) => a.sortOrder - b.sortOrder),
)
```

When `prominent` is true, render:

```svelte
<CopyableValue label={titleLabel} value={detail.entry.title} {resetToken} {onCopy} prominent />

{#if sortedFields.length > 0}
  <div class="section-label">{messages.quickAccess.usefulInformation}</div>
  {#each sortedFields as field (field.id)}
    <CopyableValue
      label={field.key}
      value={field.value}
      sensitive={field.isSensitive}
      {resetToken}
      prominent
      onCopy={(payload) => onCopy({ ...payload, label: field.key })}
    />
  {/each}
{/if}

{#if hasNotes}
  <CopyableValue
    label={notesLabel}
    value={detail.entry.notes ?? ''}
    {resetToken}
    prominent
    onCopy={(payload) => onCopy({ ...payload, label: notesLabel })}
  />
{/if}

{#each dedupedTags as tag (tag.normalizedTag)}
  <CopyableValue
    label={tag.source === 'ai' ? messages.library.aiTag : messages.library.manualTag}
    value={tag.tag}
    {resetToken}
    prominent
    onCopy={(payload) => onCopy({
      ...payload,
      label: `${tag.source === 'ai' ? messages.library.aiTag : messages.library.manualTag}：${tag.tag}`,
    })}
  />
{/each}
```

Keep the current compact markup as the `{:else}` branch so `EntryCard.svelte` is unchanged. Use `sortedFields` in both branches so input arrays are never mutated.

In `SearchMode.svelte`, pass `prominent` to `VaultEntryDetail` and split pane overflow:

```css
.left-pane { overflow: hidden; }
.right-pane { overflow-x: hidden; overflow-y: auto; }
```

- [x] **Step 6: Run focused detail and localization tests**

```powershell
pnpm test:unit -- src/lib/components/vault/CopyableValue.test.ts src/lib/components/quick-access/SearchMode.test.ts src/lib/i18n/__tests__/i18n.test.ts
pnpm check
```

Expected: all selected tests pass, field order is deterministic, and no new Svelte warning appears.

- [x] **Step 7: Commit the fast-detail slice**

```powershell
git add src/lib/components/vault/CopyableValue.svelte src/lib/components/vault/CopyableValue.test.ts src/lib/components/vault/VaultEntryDetail.svelte src/lib/components/quick-access/SearchMode.svelte src/lib/components/quick-access/SearchMode.test.ts src/lib/i18n/types.ts src/lib/i18n/locales/zh-CN.ts src/lib/i18n/locales/en.ts src/lib/i18n/__tests__/i18n.test.ts
git commit -m "promote quick access result actions"
```

---

### Task 4: Integrated UX Verification and Measured Polish

**Files:**
- Modify only after a reproduced failure: the directly responsible component/test or `src-tauri/src/lib.rs`.
- Modify: `docs/superpowers/plans/2026-07-18-quick-access-work-panel.md` — mark executed steps complete after verification.

- [x] **Step 1: Run all automated validation**

```powershell
pnpm test:unit
pnpm check
pnpm build
Push-Location src-tauri
cargo test
Pop-Location
```

Expected: all Vitest and Rust tests pass, `svelte-check` has zero errors and no new warnings in changed files, and the production frontend build completes.

- [x] **Step 2: Verify persistent cross-application copy/paste**

Run `pnpm tauri dev`, open Quick Access search, select a credential with at least account/password/URL, then perform this exact sequence:

1. Reveal password, click another application, and confirm Quick Access remains visible while the password re-masks.
2. Copy account, click the target field and paste.
3. Copy password without revealing, click the target password field and paste.
4. Copy URL, click the target field and paste.
5. Confirm query, selection, detail, and both mode drafts remain unchanged throughout.

- [x] **Step 3: Verify explicit window controls**

- Drag from title text and confirm the window moves.
- Mouse down on pin/close buttons must not drag.
- Unpin and confirm clicking another app leaves the window open but allows it to go behind.
- Pin again and confirm it stays above the target app.
- Close button and `Escape` hide without clearing state.
- Reopen and confirm the last in-memory pin state and content state remain correct.

- [x] **Step 4: Verify strict shortcut toggles for both windows**

For Quick Access, test pinned/focused, pinned/unfocused, unpinned/focused, and unpinned/unfocused states. In every visible state, one shortcut press must hide; the next press must center/show/focus.

For the main window, one main shortcut press must hide a visible window and the next press must show/focus it. Confirm the floating button still never hides Quick Access.

- [x] **Step 5: Verify detail hierarchy and target sizes**

At `680 x 480` and `480 x 340` logical sizes:

- Title appears first; account/password/URL follow stored `sortOrder`; notes and tags are last.
- Values are visually larger than before.
- Copy buttons form one right-aligned column and display localized text.
- Password reveal is immediately left of its copy button.
- Every action remains reachable and long details scroll vertically without horizontal scrolling.
- Dark and light themes retain readable text, hover states, and focus rings.

Only make a visual adjustment for a measured failure such as clipping, overlap, inaccessible action, broken alignment, or insufficient contrast. Add or update the corresponding regression test before the fix.

- [x] **Step 6: Review scope and workspace**

```powershell
git diff --check
git diff HEAD~3 --stat
git status --short
rg -n "Focused\(false\)|vault-sensitive-reset|window\.hide\(\)" src-tauri/src/lib.rs
```

Confirm:

- The focus-loss handler contains no hide call.
- Both shortcut registration paths call the same toggle helpers.
- No generated output is staged.
- The pre-existing `src-tauri/Cargo.toml` change remains unstaged and untouched.

- [x] **Step 7: Mark the implementation plan complete and commit measured polish**

Change all executed plan checkboxes to `[x]`, set an execution-complete status near the plan goal, force-add the ignored plan file, and commit it together with any measured visual adjustment:

```powershell
git add src/QuickAccessApp.svelte src/QuickAccessApp.test.ts src/lib/components/quick-access/QuickAccessWindowBar.svelte src/lib/components/quick-access/QuickAccessWindowBar.test.ts src/lib/components/quick-access/SearchMode.svelte src/lib/components/quick-access/SearchMode.test.ts src/lib/components/vault/CopyableValue.svelte src/lib/components/vault/CopyableValue.test.ts src/lib/components/vault/VaultEntryDetail.svelte src/lib/i18n/types.ts src/lib/i18n/locales/zh-CN.ts src/lib/i18n/locales/en.ts src/lib/i18n/__tests__/i18n.test.ts src-tauri/src/system/window.rs src-tauri/src/lib.rs
git add -f docs/superpowers/plans/2026-07-18-quick-access-work-panel.md
git commit -m "polish persistent quick access workflow"
```

If no visual code adjustment is needed, commit only the completed plan record; do not create an empty feature commit.
