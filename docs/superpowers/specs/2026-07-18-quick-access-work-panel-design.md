# Quick Access Persistent Work Panel Design

**Date:** 2026-07-18
**Status:** Approved design; awaiting written-spec review

## Context

Quick Access currently hides whenever its window loses focus. That behavior conflicts with the primary workflow: users need to copy several values from a search result and paste them into another application one at a time. The current search-detail hierarchy also puts notes and tags before the structured fields users usually need, while small icon buttons make repeated copying slower than necessary.

## Goals

- Keep Quick Access visible when the user clicks or switches to another application.
- Preserve sensitive-value protection by re-masking revealed values on focus loss.
- Give the borderless window explicit, mouse-accessible controls for dragging, pinning, and hiding.
- Preserve strict toggle semantics for both configured global shortcuts:
  - If the target window is visible, its shortcut hides it.
  - If the target window is hidden, its shortcut shows and focuses it.
- Put structured, directly usable search-result fields before notes and tags.
- Make result values easier to read and every copy action faster to identify and click.
- Preserve current state across focus changes and hide/show cycles.

## Non-goals

- Do not restore click-away auto-hide as an option.
- Do not add another preference or persist the pin state to SQLite.
- Do not change the main-window floating button into a toggle; it continues to open, center, and focus Quick Access.
- Do not change vault storage models, search ranking, record capture, or clipboard-clearing policy.
- Do not redesign the main Library detail density in this iteration.

## Window Interaction Model

### Focus loss

The Tauri `WindowEvent::Focused(false)` handler for `quick-access` emits `vault-sensitive-reset` but no longer calls `hide()`. Therefore:

- Clicking a target application leaves Quick Access visible.
- Revealed passwords or other sensitive fields are immediately masked again.
- Record drafts, search queries, selection, and loaded result state remain mounted and unchanged.

### Explicit hiding

Quick Access hides only through an explicit action:

- The work-panel close button.
- `Escape` while Quick Access has keyboard focus.
- The configured Quick Access global shortcut while the window is visible.

Hiding preserves the current webview and its in-progress state.

### Strict global-shortcut toggles

Shortcut behavior is based on target-window visibility, not focus or pin state.

- Quick Access shortcut:
  - visible -> hide;
  - hidden -> center, show, focus, and emit the existing input-focus event.
- Main-window shortcut:
  - visible -> hide through the existing main-window path;
  - hidden -> restore/show/focus through the existing main-window path.

The existing callbacks already implement this model and must remain regression-tested during the change. An unpinned but visible window is still considered visible, so one shortcut press hides it.

The main-window floating button is intentionally different: it always opens/focuses Quick Access and never hides it.

## Work Panel Bar

Add a compact `QuickAccessWindowBar.svelte` above the Record/Search primary switch.

- Left side: a small accent mark and localized “Quick Access” title.
- Empty title area: starts native window dragging.
- Right side: pin button followed by close button.
- Pin button:
  - Reads the real window state with `isAlwaysOnTop()` on mount.
  - Uses `setAlwaysOnTop()` to toggle it.
  - Default is pinned because the Tauri window configuration remains `alwaysOnTop: true`.
  - The state remains in memory for the life of the window but is not added to preferences.
- Close button calls `hide()` and exposes a localized accessible name.
- Button clicks never start dragging.
- Pin and close buttons have at least a 30 logical-pixel hit target, visible hover/focus states, and theme-token colors.

`QuickAccessApp.svelte` owns the native-window actions and passes deterministic callbacks/state into the presentation component. A failed pin operation restores the previous state and produces the existing in-window error notice rather than leaving a false visual state.

The title bar adds only a compact row; the approved `680 x 480` target and `480 x 340` minimum remain unchanged unless real runtime testing proves a concrete clipping failure.

## Search Detail Hierarchy

`VaultEntryDetail.svelte` gains an explicit Quick Access presentation mode. The main Library retains its existing compact density.

Quick Access order is:

1. Entry title.
2. Structured fields sorted by `sortOrder` (account, password, URL, API key, and similar directly usable values).
3. Notes, when present.
4. Tags and other supporting metadata.

This is a presentation-only ordering change. It does not infer field importance from localized names and does not mutate stored order.

The right search-detail pane scrolls vertically so long field sets remain reachable at both supported window sizes.

## Fast Copy Presentation

`CopyableValue.svelte` gains a prominent-action variant used only by Quick Access search details.

- Value text is approximately 14 logical pixels and uses the existing configured font stack.
- Labels remain quieter than values but increase enough to scan comfortably.
- Rows gain a larger minimum height and spacing.
- Copy becomes a visible localized text button, approximately `52 x 32` logical pixels.
- Sensitive rows render actions in this fixed order: reveal/hide, then copy.
- Copy is always the last control and therefore forms one right-aligned column across all rows.
- Reveal/hide has the same height as copy and at least a 32-pixel square hit target.
- Title, notes, and tags remain independently copyable but appear after the directly usable fields where applicable.
- Keyboard focus styles and existing accessible labels remain intact.
- Copying never reveals a sensitive value and never hides the panel.

All colors, radii, spacing, fonts, hover states, and focus rings continue to derive from the shared theme tokens.

## Component Changes

### New component

`src/lib/components/quick-access/QuickAccessWindowBar.svelte`

- Props: `pinned`, `pinPending`, `onTogglePin`, `onHide`, and `onDrag`.
- Owns markup, icons, labels, tooltips, and presentation only.

### Modified components

- `QuickAccessApp.svelte`
  - Renders the work-panel bar.
  - Initializes and toggles actual always-on-top state.
  - Retains `Escape` hiding and mode/state preservation.
- `SearchMode.svelte`
  - Requests the prominent Quick Access detail mode.
  - Makes the detail pane vertically scrollable.
- `VaultEntryDetail.svelte`
  - Provides deterministic Quick Access grouping and field ordering.
  - Keeps the current compact presentation as the default.
- `CopyableValue.svelte`
  - Adds the prominent variant and guarantees copy-last action order.
- Locale contracts and both locale files
  - Add only labels that cannot be accurately reused, such as the usable-information section heading.
- `src-tauri/src/lib.rs`
  - Stops hiding Quick Access on focus loss while retaining the sensitive-reset event.
  - Leaves both global-shortcut visibility toggles intact.

## Error Handling

- A pin/unpin failure keeps the last confirmed state and shows an error notice.
- Hide and drag failures are logged or surfaced through the existing notice path without changing content state.
- Clipboard failures continue using the current localized copy-failure notification.
- Focus-loss masking is independent from hide behavior, so removing auto-hide cannot expose an already revealed value after switching applications.

## Testing

### Component tests

- Work-panel title, pin, and close controls expose localized accessible names.
- Dragging starts only from non-button title-bar space.
- Pin and close callbacks fire once and pending pin state blocks duplicate activation.
- Prominent copy buttons show localized text and retain value-free accessible names.
- Sensitive rows render reveal before copy, with copy as the final action.
- Window blur and reset-token changes still re-mask sensitive values.

### Detail tests

- Quick Access DOM order is title -> sorted fields -> notes -> tags.
- Stored `sortOrder` determines field order without mutating the input array.
- Every title, field, note, and tag remains independently copyable.
- The right pane remains mounted after repeated copy actions.

### Window and shortcut verification

- Focus loss emits sensitive reset without hiding Quick Access.
- Quick Access shortcut hides a visible window and opens/focuses a hidden window.
- Main shortcut hides a visible main window and restores/focuses a hidden main window.
- Shortcut behavior remains visibility-based when Quick Access is unpinned and unfocused.
- Floating-button activation always opens/focuses and never toggles closed.

### Validation commands

- `pnpm test:unit`
- `pnpm check`
- `pnpm build`
- `cargo test` from `src-tauri/`
- Manual `pnpm tauri dev` verification of repeated copy/paste across applications, focus-loss masking, drag, pin/unpin, close, `Escape`, both global shortcuts, and target/minimum window sizes.

## Acceptance Criteria

- Quick Access stays visible during repeated cross-application copy/paste work.
- Losing focus re-masks sensitive values without clearing or hiding anything.
- The work-panel bar makes drag, pin state, and explicit hiding immediately discoverable.
- The Quick Access shortcut strictly toggles Quick Access visibility.
- The main shortcut strictly toggles main-window visibility by the same rule.
- The floating main-window button remains open/focus-only.
- Search detail shows structured fields before notes and tags.
- Search values and copy controls are visibly larger, with every copy control aligned at the far right.
- Password reveal precedes copy, while copy remains the rightmost action.
- Long details remain scrollable and usable at `680 x 480` and `480 x 340` logical pixels.
- Existing search, capture, state preservation, localization, theme, keyboard, and accessibility behavior continue to pass.
