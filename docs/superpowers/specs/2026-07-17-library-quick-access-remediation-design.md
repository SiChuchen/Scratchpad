# Library and Quick Access Remediation Design

## Goal

Make the Library and global Quick Access flows safe and predictable before release. The remediation covers every issue found in the 2026-07-17 review: broken settings navigation, state loss between Quick Access modes, stale entry editing, sensitive error disclosure, duplicate credential fields, missing focus recovery, incomplete disabled-AI recovery, incomplete English UI, and formatting/test gates.

## User Mental Model

- "Configure now" takes the user directly to the relevant settings in a visible main window.
- Switching tabs changes context without discarding unfinished work.
- After switching modes, typing continues in the primary input without another click.
- An editor always represents the entry whose edit action was most recently chosen.
- Credential values never appear in summaries, error messages, notifications, or other non-secret UI.
- Equivalent field labels such as `user` and `Username` produce one field rather than duplicates.
- A disabled AI feature always has a visible recovery action.
- Selecting English produces English user-visible text throughout the affected flows.

## Architecture

### Main-window settings navigation

Add a dedicated Tauri command for opening settings from Quick Access. The command will:

1. Show and focus the `main` window.
2. Emit a main-window event requesting the Settings view.
3. Hide the `quick-access` window.

`App.svelte` will register and clean up a listener for that event and call the existing `navigate('settings')` function. This keeps window ownership in Rust and view ownership in the main Svelte application. The existing `ipc_open_quick_access` toggle remains unchanged for its original toolbar and shortcut purpose.

### Quick Access mode lifetime and focus

Both `CaptureMode` and `SearchMode` will remain mounted for the lifetime of the Quick Access webview. The inactive panel will be hidden rather than destroyed. This preserves the complete internal state of each workflow, including parsed drafts, enrichment results, search results, selection, and loaded detail, without exposing a large controlled-state API or introducing a global store.

All mouse and keyboard mode changes will go through one `switchMode` function. It will update the active mode and schedule focus on that mode's primary input. The tab markup will expose stable tab/panel relationships and the inactive panel will not be focusable or visible to assistive technology.

The AI status banner will show the settings action whenever AI is unavailable for automatic enrichment, whether the cause is missing configuration or the `autoEnrich` preference being disabled.

### Entry editor identity

The edit-mode `VaultEntryEditor` will be keyed by entry ID. Selecting another entry therefore destroys the stale editor and constructs a new editor from the newly selected detail. Create mode remains separate. This preserves the editor's current local-state design while guaranteeing that displayed form data and save target have the same identity.

### Sensitive error handling

Sensitive-metadata validation will return a generic, localized-safe error that contains no field value. Tests will assert both the expected failure and the absence of the original password/token from the returned error. The frontend may display this error because the backend contract guarantees that it is safe for user-visible surfaces.

The existing conservative leak detection policy remains in place. This remediation changes disclosure behavior, not the security boundary.

### Credential field canonicalization

Field merging will use a small, explicit alias map before comparison. Initial aliases cover common credential labels only:

- `user`, `username`, `login`, `login_name`, and `用户名` -> `user`
- `password`, `passwd`, `pwd`, and `密码` -> `password`

Unknown field names continue to use trimmed, case-insensitive comparison. The original displayed key of the existing local field is retained so the merge does not unexpectedly rename user input.

### Internationalization

User-visible strings in the affected Library and Settings paths will move into the existing typed locale objects. Dynamic notifications will be assembled from localized prefixes and labels. Tests will continue to enforce locale shape parity and will add coverage for the new message keys.

### Formatting

Run Rust formatting across the package after behavioral changes. Formatting-only changes remain mechanical and are not mixed with unrelated refactoring.

## Error Handling

- Failure to open the main settings window produces a visible error notice in Quick Access; success does not show a redundant toast because the destination itself confirms the action.
- Existing capture work remains available if settings navigation fails.
- Entry save failures keep the editor open and preserve its content.
- Sensitive validation errors identify the failed policy without identifying the sensitive value.

## Test Strategy

Each behavioral change follows red-green-refactor:

1. Component-level test for Quick Access settings navigation and failure notice.
2. Component-level test proving record and search state survive mode changes and the new input receives focus.
3. Component-level test proving switching edit target replaces editor state and save targets the matching entry.
4. Rust regression test proving sensitive leak errors exclude the secret.
5. Controller test proving credential aliases merge without duplicates.
6. Component/i18n tests for the auto-enrich-disabled recovery action and English text.
7. Full gates: `pnpm test:unit`, `pnpm check`, `pnpm build`, `cargo fmt -- --check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings`.
8. Isolated-data Tauri smoke test covering global shortcut, mode switching, draft retention, settings navigation, entry switching, and minimum-size behavior.

## Non-goals

- Replacing component-local state with a global store.
- Redesigning the Library or Quick Access visual language.
- Weakening sensitive metadata validation.
- Refactoring unrelated pre-existing accessibility warnings or application modules.

## Acceptance Criteria

- Clicking "Configure now" always leaves the main Settings view visible and focused.
- Record and search work survives repeated mouse and keyboard tab switches.
- The active mode input receives focus after every switch.
- Editing A and then selecting B can never save A's form values to B.
- No sensitive value is present in a metadata-validation error string or rendered alert.
- Common username/password aliases merge into one logical field.
- Missing AI configuration and disabled auto-enrichment both expose the same recovery path.
- The affected UI has no hardcoded Chinese user-visible strings.
- All automated and formatting gates pass, followed by a successful isolated desktop smoke test.
