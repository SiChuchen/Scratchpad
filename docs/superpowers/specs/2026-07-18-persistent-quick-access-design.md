# Persistent Quick Access Entry Design

**Date:** 2026-07-18
**Status:** Approved for implementation

## Context

The application already has a separate Quick Access window with two modes: record and search. It can be opened from the Library page or with a global shortcut. Two usability issues remain:

1. Mouse users cannot reach Quick Access consistently from every main-window page.
2. Record and Search are the only two primary modes in the Quick Access window, but the current small text tabs make them look like secondary navigation.

## Goals

- Keep one circular Quick Access entry visible on every normal page in the main window.
- Open the existing Quick Access window without changing the current main-window page.
- Preserve the Quick Access window's current mode and in-progress state.
- Make switching between Record and Search the strongest navigation element in that window.
- Keep the Quick Access window on the exact same live theme as the main window, including theme changes made while both windows are visible.
- Use a compact default window size that gives both modes enough working space without the current excessive empty area.
- Preserve keyboard access, localization, theming, and screen-reader semantics.

## Non-goals

- No desktop-level floating window when the main window is hidden or minimized; the existing global shortcut covers that workflow.
- No new record/search implementation and no backend state model changes.
- No mode-selection landing page that would add an extra click.
- No draggable or user-configurable floating-button position in this iteration.

## Interaction Design

### Main-window floating entry

Add a circular floating action button to the top-level `App.svelte` shell so it survives all `currentView` changes (`home`, `categories`, `note`, `vault`, and `settings`).

- Position: bottom-right inside the main window, with a consistent inset from both edges.
- Form: circular, approximately 48 px, using the primary theme color and a lightning/quick-action icon.
- Labeling: localized accessible name and tooltip describing “Open quick access”.
- Action: invoke the existing `ipc_open_quick_access` command.
- State: prevent duplicate invocations while an open request is in flight.
- Failure: show a localized error toast in the main window and restore the enabled state.
- Layering: appear above normal page content and menus, but below blocking confirmation and drag/drop overlays.
- Visibility: remain present on all normal pages; blocking overlays may cover it because they intentionally suspend page interaction.

The existing rectangular Quick Access button in `VaultView.svelte` will be removed. Keeping both controls on the Library page would create two equivalent calls to action and make the persistent control look accidental rather than global.

### Quick Access primary mode navigation

Replace the visually small tab treatment with a full-width, two-segment primary mode switch at the top of the Quick Access window.

- Record and Search each occupy half of the available width.
- Each target is approximately 46-48 px high and contains an icon plus a visible text label.
- The selected mode uses a filled primary-tinted surface, stronger text, and a clear accent edge.
- The unselected mode remains visibly clickable with sufficient contrast and a hover state.
- The switch remains compact enough to preserve workspace for the active form or results.
- On narrow windows, the two equal segments remain side by side rather than collapsing into an ambiguous menu.

Existing behavior remains unchanged:

- `Ctrl+Tab` switches modes.
- `Escape` hides the window.
- Both mode components stay mounted so switching or hiding the window does not discard input or search state.
- Existing `role="tablist"`, `role="tab"`, `aria-selected`, `aria-controls`, and focus behavior are retained.

### Shared live theme

The main and Quick Access windows use the same `DockPreferences`, `computeThemeTokens`, font variables, locale state, and system-dark-mode source. They must not develop a second Quick Access-specific palette.

- `App.svelte` broadcasts each optimistic preference change as soon as the main window applies it, before the debounced persistence completes.
- `QuickAccessApp.svelte` listens for that cross-window event and replaces its preference snapshot immediately.
- The existing preference reload when Quick Access is shown remains as recovery for events missed while the window was unavailable.
- Both windows continue listening to `prefers-color-scheme` so system-theme changes produce the same token set.
- If persistence fails, both windows still match the same optimistic main-window state and the existing main-window save error remains visible.

### Window dimensions

The initial target is **680 × 480 logical pixels**, with a **480 × 340 logical-pixel minimum**. Runtime sizing converts these logical targets using the window scale factor before centering and clamping them to 90% of the active monitor work area. This keeps the perceived size consistent on common Windows DPI settings.

The target will be validated at 100%, 125%, and 150% scaling and at the minimum, target, and small-monitor-clamped sizes. Increase only the dimension that fails a concrete criterion (clipping, overlapping controls, unusable result list, or inaccessible primary action); do not enlarge the window merely to preserve empty space.

## Component and Data Flow

### New component

Create `src/lib/components/QuickAccessFab.svelte` as a small presentation component.

- Props: `onOpen` callback and `disabled` state.
- Owns the circular button markup, icon, tooltip, hover/pressed/focus appearance, and accessible label.
- Does not call Tauri directly, which keeps it deterministic and easy to test.

### Main shell

`App.svelte` owns the asynchronous open operation:

1. Ignore another click while an open request is pending.
2. Set the pending state.
3. Invoke `ipc_open_quick_access`.
4. On failure, show the localized toast.
5. Clear the pending state in `finally`.

### Quick Access window

`QuickAccessApp.svelte` retains its existing `mode` state and `switchMode` behavior. Only the mode-switch markup styling and icons change; Record/Search contents and command flows remain untouched.

It also subscribes to the shared preference-change event and runs the same theme-token application path used for its initial load and show-time refresh.

### Library page cleanup

Remove the Library-only Quick Access button, its local handler, and any now-unused import/style declarations from `VaultView.svelte`.

## Accessibility and User Experience Constraints

- The floating control must be a native `button`, keyboard reachable, and expose a visible focus indicator.
- Its accessible name must describe the outcome rather than the icon.
- The icon is decorative and hidden from assistive technology.
- Disabled/pending behavior must not permanently trap the button after an error.
- The stronger mode switch must not rely on color alone; selected state also uses shape/edge treatment and ARIA state.
- Motion is limited to short hover/press transitions and respects `prefers-reduced-motion`.
- The floating control must not replace the global shortcut; both routes lead to the same preserved Quick Access window.
- Default and minimum Quick Access dimensions must remain usable without horizontal scrolling or clipped actions.

## Localization

Reuse existing Record, Search, and Open Quick Access labels where their meaning fits. Add a localized main-window failure message if no existing message accurately describes failure to open the Quick Access window. Update the i18n type contract and both Chinese and English locale files together.

## Testing

### Component tests

- The floating control renders with a localized accessible name.
- A click calls `onOpen` once.
- Disabled/pending state blocks repeated activation.

### Quick Access tests

- Record and Search remain semantic tabs with correct selected state.
- Clicking each prominent segment switches the visible panel.
- Existing keyboard switching and state-preservation tests continue to pass.
- A cross-window preference event updates the Quick Access theme and font variables without reopening it.

### Window sizing tests

- Pure Rust tests cover target logical size conversion at 100%, 125%, and 150% scale factors.
- Small work areas still clamp the result to 90% and center it, including negative-coordinate monitors.
- Runtime minimum dimensions are scale-aware and never exceed the clamped work area.

### Main-shell/integration verification

- The control is mounted outside the conditional view branch and therefore remains visible after navigation.
- Clicking it invokes `ipc_open_quick_access` without changing `currentView`.
- An invocation error produces the user-facing toast and re-enables the button.
- The Library page has only the global floating entry, not a duplicate button.

### Validation commands

- `pnpm test:unit`
- `pnpm check`
- `pnpm build`
- `cargo test` from `src-tauri/`
- Manual Tauri verification across every main-window page, all built-in themes, live theme switching while Quick Access is visible, 100%/125%/150% DPI, target/minimum/clamped sizes, keyboard navigation, and repeated open/hide cycles.

## Acceptance Criteria

- A circular Quick Access button is continuously visible on all main-window pages.
- Clicking it opens the existing Quick Access window and preserves both windows' current state.
- Record/Search switching is the most visually prominent control in Quick Access.
- Quick Access changes theme and fonts together with the main window without requiring hide/show or restart.
- The default window is compact and balanced at 680 × 480 logical pixels, while the minimum and clamped sizes keep all primary actions reachable.
- No duplicate Quick Access action remains on the Library page.
- Mouse, keyboard, shortcut, localization, theme, and accessibility behavior all remain functional.
