# Library and Quick Access Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Library and global Quick Access workflows preserve user work, navigate predictably, edit the correct record, and never disclose sensitive values in UI errors.

**Architecture:** Keep both Quick Access modes mounted and centralize mode switching/focus in `QuickAccessApp`; add a dedicated Rust-to-main-window settings event; key edit components by entry ID; keep sensitive validation errors code-based; canonicalize only a conservative set of credential field aliases. Reuse the existing typed i18n objects and component test stack.

**Tech Stack:** Svelte 5 runes, TypeScript, Vitest, Testing Library, Tauri 2, Rust, Cargo.

---

## File Map

- Create `src/QuickAccessApp.test.ts`: root-window mode lifetime, focus, and settings-command coverage.
- Modify `src/QuickAccessApp.svelte`: dedicated settings command, persistent panels, tab semantics, centralized focus.
- Modify `src/App.svelte`: listen for the main-window settings navigation event.
- Modify `src-tauri/src/lib.rs`: register `ipc_open_main_settings`, show/focus main, emit navigation, hide Quick Access.
- Modify `src/lib/components/quick-access/CaptureMode.test.ts`: disabled-AI recovery action and safe error rendering.
- Modify `src/lib/components/quick-access/CaptureMode.svelte`: always expose recovery action and localize safe validation errors.
- Create `src/lib/components/views/VaultView.test.ts`: regression for switching edit targets and save identity.
- Modify `src/lib/components/views/VaultView.svelte`: key edit editor by entry ID and use localized notifications.
- Modify `src/lib/state/capture-draft.test.ts`: credential-alias merge regressions.
- Modify `src/lib/state/capture-draft.ts`: conservative field-key canonicalization.
- Modify `src-tauri/src/vault/ipc/capture.rs`: return a stable error code without the sensitive value and test it.
- Modify `src/lib/i18n/types.ts`: typed message keys for Quick Access, Library, and data-directory UI.
- Modify `src/lib/i18n/locales/zh-CN.ts`: Chinese values for the new keys.
- Modify `src/lib/i18n/locales/en.ts`: English values for the new keys.
- Modify `src/lib/i18n/__tests__/i18n.test.ts`: locale parity and English regression assertions.
- Modify `src/lib/components/views/SettingsView.svelte`: remove hardcoded Chinese from the affected path.
- Format Rust files under `src-tauri/src/` mechanically with `cargo fmt`.

### Task 1: Open the visible main Settings view from Quick Access

**Files:**
- Create: `src/QuickAccessApp.test.ts`
- Modify: `src/QuickAccessApp.svelte:18-80,178-256`
- Modify: `src/App.svelte:18-30,381-399`
- Modify: `src-tauri/src/lib.rs:379-393,790-820`

- [ ] **Step 1: Write the failing Quick Access command test**

Mock `@tauri-apps/api/core`, `@tauri-apps/api/window`, and `@tauri-apps/api/event`, render `QuickAccessApp`, then assert that the recovery button invokes the dedicated command rather than the Quick Access toggle:

```ts
it('opens the main Settings view through the dedicated command', async () => {
  render(QuickAccessApp)
  await fireEvent.click(await screen.findByRole('button', { name: '立即配置' }))

  expect(mockInvoke).toHaveBeenCalledWith('ipc_open_main_settings')
  expect(mockInvoke).not.toHaveBeenCalledWith('ipc_open_quick_access')
})
```

Configure mocked preferences and AI calls so the component reaches the unconfigured state. Keep the real `CaptureMode` mounted; mock only Tauri boundaries and `vaultApi` network calls.

- [ ] **Step 2: Run the test and verify RED**

Run:

```powershell
pnpm test:unit -- src/QuickAccessApp.test.ts
```

Expected: FAIL because the current button invokes `ipc_open_quick_access`.

- [ ] **Step 3: Add the dedicated backend command**

Add and register:

```rust
#[tauri::command]
fn ipc_open_main_settings(app: tauri::AppHandle) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    main.show().map_err(|e| e.to_string())?;
    main.set_focus().map_err(|e| e.to_string())?;
    app.emit("main-open-settings", ()).map_err(|e| e.to_string())?;
    if let Some(quick) = app.get_webview_window("quick-access") {
        quick.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}
```

Import the Tauri emitter trait if the existing imports do not already provide it. Do not change `ipc_open_quick_access`.

- [ ] **Step 4: Route the event in the main application**

Inside the existing `App.svelte` mount lifecycle, register and clean up:

```ts
const unlistenOpenSettings = await listen('main-open-settings', () => {
  navigate('settings')
})
```

Add `unlistenOpenSettings()` to teardown. Change `onOpenAiSettings` to await `ipc_open_main_settings`; only show an error notice on rejection.

- [ ] **Step 5: Run focused tests and compile checks**

```powershell
pnpm test:unit -- src/QuickAccessApp.test.ts
pnpm check
cargo test
```

Expected: focused test passes, Svelte has zero errors, Rust tests pass.

- [ ] **Step 6: Commit the navigation fix**

```powershell
git add src/QuickAccessApp.test.ts src/QuickAccessApp.svelte src/App.svelte src-tauri/src/lib.rs
git commit -m "fix quick access settings navigation"
```

### Task 2: Preserve both Quick Access workflows and restore focus

**Files:**
- Modify: `src/QuickAccessApp.test.ts`
- Modify: `src/QuickAccessApp.svelte:35-47,178-256`

- [ ] **Step 1: Add failing state-retention and focus tests**

Use the real child components with mocked APIs. Enter a record draft, switch to Search, enter a query, then switch in both directions:

```ts
it('preserves each mode and focuses its primary input after switching', async () => {
  render(QuickAccessApp)
  const recordInput = await screen.findByRole('textbox', { name: '记录' })
  await fireEvent.input(recordInput, { target: { value: 'UXREVIEWDRAFT' } })

  await fireEvent.click(screen.getByRole('tab', { name: '搜索' }))
  const searchInput = screen.getByRole('searchbox', { name: '搜索' })
  expect(searchInput).toHaveFocus()
  await fireEvent.input(searchInput, { target: { value: 'database' } })

  await fireEvent.click(screen.getByRole('tab', { name: '记录' }))
  expect(recordInput).toHaveValue('UXREVIEWDRAFT')
  expect(recordInput).toHaveFocus()

  await fireEvent.keyDown(window, { key: 'Tab', ctrlKey: true })
  expect(searchInput).toHaveValue('database')
  expect(searchInput).toHaveFocus()
})
```

Also assert `aria-controls`, matching tabpanel IDs, and that the inactive panel has `hidden`.

- [ ] **Step 2: Run the test and verify RED**

```powershell
pnpm test:unit -- src/QuickAccessApp.test.ts
```

Expected: FAIL because the conditional destroys inactive components and tab changes do not focus inputs.

- [ ] **Step 3: Keep both modes mounted**

Replace the conditional component block with stable panels:

```svelte
<section id="qa-record-panel" role="tabpanel" aria-labelledby="qa-record-tab" hidden={mode !== 'record'}>
  <CaptureMode ... />
</section>
<section id="qa-search-panel" role="tabpanel" aria-labelledby="qa-search-tab" hidden={mode !== 'search'}>
  <SearchMode ... />
</section>
```

Give each tab a stable ID and `aria-controls`. Remove the unused root `draft`, `query`, and `selectedId` variables and correct comments that claim those dummy fields clear child state.

- [ ] **Step 4: Centralize all mode changes**

Add:

```ts
function switchMode(next: 'record' | 'search') {
  mode = next
  focusActiveModeInput()
}
```

Use it from tab clicks and from the existing keyboard action result. Preserve Escape behavior. Because `focusActiveModeInput` schedules a microtask, it will run after `hidden` updates.

- [ ] **Step 5: Run focused tests and check**

```powershell
pnpm test:unit -- src/QuickAccessApp.test.ts
pnpm check
```

Expected: state and focus tests pass with zero Svelte errors.

- [ ] **Step 6: Commit the mode-lifetime fix**

```powershell
git add src/QuickAccessApp.test.ts src/QuickAccessApp.svelte
git commit -m "preserve quick access mode state"
```

### Task 3: Keep editor content and save target on the same entry

**Files:**
- Create: `src/lib/components/views/VaultView.test.ts`
- Modify: `src/lib/components/views/VaultView.svelte:456-472`

- [ ] **Step 1: Write the failing editor-identity test**

Mock `vaultApi.listEntries`, `getEntry`, `getLlmConfig`, `getAiSettings`, and event wrappers. Return two summaries and details titled `Entry A` and `Entry B`. Render the real `VaultView` and execute:

```ts
await fireEvent.click(await screen.findByRole('button', { name: '编辑 Entry A' }))
expect(screen.getByDisplayValue('Entry A')).toBeInTheDocument()

await fireEvent.click(screen.getByRole('button', { name: '编辑 Entry B' }))
expect(screen.getByDisplayValue('Entry B')).toBeInTheDocument()
expect(screen.queryByDisplayValue('Entry A')).not.toBeInTheDocument()

await fireEvent.submit(screen.getByDisplayValue('Entry B').closest('form')!)
expect(mockVaultApi.updateEntry).toHaveBeenCalledWith(
  'entry-b',
  expect.objectContaining({ title: 'Entry B' }),
)
```

- [ ] **Step 2: Run the test and verify RED**

```powershell
pnpm test:unit -- src/lib/components/views/VaultView.test.ts
```

Expected: FAIL because the editor component keeps A's locally captured state after the parent selects B.

- [ ] **Step 3: Key edit mode by entry ID**

Wrap only the edit editor:

```svelte
{#key editorMode.id}
  <VaultEntryEditor
    mode="edit"
    initial={editorMode.detail}
    onSave={handleSaveEdit}
    onCancel={cancelEditor}
    onRemoveAiTag={handleRemoveAiTag}
  />
{/key}
```

Do not add prop-reset effects inside `VaultEntryEditor`; component identity belongs to the parent route state.

- [ ] **Step 4: Run focused tests and Svelte check**

```powershell
pnpm test:unit -- src/lib/components/views/VaultView.test.ts
pnpm check
```

Expected: the form switches to B and saves B. The existing local-state warnings may remain for create/edit initialization but no new warnings are introduced.

- [ ] **Step 5: Commit the editor safety fix**

```powershell
git add src/lib/components/views/VaultView.test.ts src/lib/components/views/VaultView.svelte
git commit -m "prevent stale library editor state"
```

### Task 4: Remove sensitive values from validation errors

**Files:**
- Modify: `src-tauri/src/vault/ipc/capture.rs:222-268,595-624`
- Modify: `src/lib/components/quick-access/CaptureMode.test.ts`
- Modify: `src/lib/components/quick-access/CaptureMode.svelte:328-345`

- [ ] **Step 1: Write the failing Rust disclosure test**

Extend the existing sensitive metadata tests:

```rust
#[test]
fn sensitive_metadata_error_never_contains_the_sensitive_value() {
    let secret = "DO_NOT_ECHO_THIS_PASSWORD";
    let mut draft = sample_draft();
    draft.fields.push(FieldInput {
        key: "password".into(),
        value: secret.into(),
        is_sensitive: true,
    });
    draft.ai_tags = vec![secret.into()];
    let err = reject_sensitive_metadata_leak(&draft)
        .expect_err("metadata leak must be rejected");

    assert_eq!(err, "sensitive_metadata_rejected");
    assert!(!err.contains(secret));
}
```

Reuse the existing `sample_draft`, `FieldInput`, and `reject_sensitive_metadata_leak` test scope; do not create a second leak checker.

- [ ] **Step 2: Run the Rust test and verify RED**

```powershell
cargo test sensitive_metadata_error_never_contains_the_sensitive_value
```

Expected: FAIL because the current error includes the sensitive value.

- [ ] **Step 3: Return a stable non-secret error code**

Replace both value-bearing branches with:

```rust
return Err("sensitive_metadata_rejected".to_string());
```

Keep the existing long/short matching policy unchanged.

- [ ] **Step 4: Add the failing frontend localization test**

In `CaptureMode.test.ts`, reject `createFromCapture` with `sensitive_metadata_rejected` and assert that the rendered alert contains a friendly localized message and not the fixture secret or raw error code.

- [ ] **Step 5: Map the stable code in CaptureMode**

Add a small formatter:

```ts
function captureSaveError(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error)
  return raw === 'sensitive_metadata_rejected'
    ? messages.quickAccess.sensitiveMetadataRejected
    : raw
}
```

Use it in the save catch branch.

- [ ] **Step 6: Run focused frontend and Rust tests**

```powershell
cargo test sensitive_metadata_error_never_contains_the_sensitive_value
pnpm test:unit -- src/lib/components/quick-access/CaptureMode.test.ts
```

Expected: both pass and neither DOM nor error string contains the secret.

- [ ] **Step 7: Commit the disclosure fix**

```powershell
git add src-tauri/src/vault/ipc/capture.rs src/lib/components/quick-access/CaptureMode.test.ts src/lib/components/quick-access/CaptureMode.svelte
git commit -m "hide sensitive values in capture errors"
```

### Task 5: Merge equivalent credential field names

**Files:**
- Modify: `src/lib/state/capture-draft.test.ts:180-240`
- Modify: `src/lib/state/capture-draft.ts:250-290`

- [ ] **Step 1: Write failing alias tests**

Add separate username and password cases:

```ts
it.each([
  ['Username', 'user'],
  ['login_name', 'user'],
  ['用户名', 'user'],
  ['passwd', 'password'],
  ['pwd', 'password'],
  ['密码', 'password'],
])('merges suggested %s into local %s', (suggestedKey, localKey) => {
  const controller = controllerWithLocalField(localKey)
  controller.applyEnrichment(enrichmentWithField(suggestedKey))

  expect(controller.current!.fields).toHaveLength(1)
  expect(controller.current!.fields[0]!.key).toBe(localKey)
})
```

Use existing fixtures and controller construction from the test file instead of adding test-only production APIs.

- [ ] **Step 2: Run the tests and verify RED**

```powershell
pnpm test:unit -- src/lib/state/capture-draft.test.ts
```

Expected: alias cases fail because current normalization only lowercases the key.

- [ ] **Step 3: Add conservative canonicalization**

Add a module-local helper:

```ts
const FIELD_KEY_ALIASES: Readonly<Record<string, string>> = {
  user: 'user',
  username: 'user',
  login: 'user',
  login_name: 'user',
  用户名: 'user',
  password: 'password',
  passwd: 'password',
  pwd: 'password',
  密码: 'password',
}

function canonicalFieldKey(key: string): string {
  const normalized = key.trim().toLowerCase()
  return FIELD_KEY_ALIASES[normalized] ?? normalized
}
```

Use `canonicalFieldKey` for both local and suggested field lookup. Keep the existing local field's display key.

- [ ] **Step 4: Run focused and related tests**

```powershell
pnpm test:unit -- src/lib/state/capture-draft.test.ts src/lib/components/quick-access/CaptureMode.test.ts
```

Expected: all alias and capture-controller tests pass.

- [ ] **Step 5: Commit the merge fix**

```powershell
git add src/lib/state/capture-draft.test.ts src/lib/state/capture-draft.ts
git commit -m "merge common credential field aliases"
```

### Task 6: Complete recovery actions and affected-flow internationalization

**Files:**
- Modify: `src/lib/components/quick-access/CaptureMode.test.ts`
- Modify: `src/lib/components/quick-access/CaptureMode.svelte:376-391`
- Modify: `src/lib/components/views/VaultView.test.ts`
- Modify: `src/lib/components/views/VaultView.svelte:230-317,369-377`
- Modify: `src/lib/components/views/SettingsView.svelte:44-64,388-401`
- Modify: `src/lib/i18n/types.ts:10-67,141-196`
- Modify: `src/lib/i18n/locales/zh-CN.ts`
- Modify: `src/lib/i18n/locales/en.ts`
- Modify: `src/lib/i18n/__tests__/i18n.test.ts`

- [ ] **Step 1: Add failing recovery and English tests**

Add a CaptureMode test that renders `aiConfigured=true` and `autoEnrich=false`, then expects the Configure button and verifies `onOpenSettings` is called.

Add English assertions for the new locale keys:

```ts
expect(en.quickAccess.autoEnrichDisabled).toBe('AI auto-organization is off')
expect(en.quickAccess.configureNow).toBe('Configure now')
expect(en.settings.selectDataDirTitle).toBe('Select data directory')
expect(en.library.saved).toBe('Saved')
```

Extend `VaultView.test.ts` under English locale and assert that save/copy/failure notifications use English prefixes.

- [ ] **Step 2: Run focused tests and verify RED**

```powershell
pnpm test:unit -- src/lib/components/quick-access/CaptureMode.test.ts src/lib/components/views/VaultView.test.ts src/lib/i18n/__tests__/i18n.test.ts
```

Expected: missing configuration action and locale keys cause failures.

- [ ] **Step 3: Extend typed locales**

Add these keys with Chinese and English values:

```ts
quickAccess: {
  aiNotConfigured: string
  autoEnrichDisabled: string
  configureNow: string
  openSettingsFailed: string
  sensitiveMetadataRejected: string
}
library: {
  created: string
  saved: string
  removeTagFailed: string
  aiError: string
  openQuickAccess: string
  quickAccess: string
}
settings: {
  loading: string
  selectDataDirTitle: string
  changeDataDirFailed: string
}
```

Reuse `messages.toast.loadFailed/createFailed/saveFailed/copyFailed` and the existing `library.copiedLabel` where a suitable localized message already exists; do not duplicate generic keys.

- [ ] **Step 4: Remove affected hardcoded strings**

In `CaptureMode`, show the settings button for `!aiConfigured || !autoEnrich` and use only locale messages.

In `VaultView`, replace the hardcoded load/create/save/tag/copy/AI notifications and Quick Access button text/title with locale messages and placeholder replacement.

In `SettingsView`, localize the directory picker title, directory-change failure, and loading label.

- [ ] **Step 5: Run focused tests and scan for remaining affected strings**

```powershell
pnpm test:unit -- src/lib/components/quick-access/CaptureMode.test.ts src/lib/components/views/VaultView.test.ts src/lib/i18n/__tests__/i18n.test.ts
rg -n "加载失败|已创建|创建失败|已保存|保存失败|移除标签失败|已复制|复制失败|AI 错误|选择数据目录|更改数据目录失败|加载中|立即配置|自动整理已关闭" src/QuickAccessApp.svelte src/lib/components/quick-access/CaptureMode.svelte src/lib/components/views/VaultView.svelte src/lib/components/views/SettingsView.svelte
```

Expected: tests pass; the scan finds no user-visible hardcoded occurrences in those components.

- [ ] **Step 6: Commit the recovery/i18n fix**

```powershell
git add src/lib/components/quick-access/CaptureMode.test.ts src/lib/components/quick-access/CaptureMode.svelte src/lib/components/views/VaultView.test.ts src/lib/components/views/VaultView.svelte src/lib/components/views/SettingsView.svelte src/lib/i18n/types.ts src/lib/i18n/locales/zh-CN.ts src/lib/i18n/locales/en.ts src/lib/i18n/__tests__/i18n.test.ts
git commit -m "complete library recovery and translations"
```

### Task 7: Format and run the full acceptance gate

**Files:**
- Mechanically modify: Rust files selected by `cargo fmt`
- Create: `docs/superpowers/verification/2026-07-17-library-quick-access-remediation.md`

- [ ] **Step 1: Format Rust and verify the formatting gate**

```powershell
cargo fmt
cargo fmt -- --check
```

Run from `src-tauri/`. Expected: the check exits 0. Review the formatting diff to ensure it is mechanical.

- [ ] **Step 2: Run all frontend gates fresh**

```powershell
pnpm test:unit
pnpm check
pnpm build
```

Expected: all unit tests pass, Svelte reports zero errors, and the production build exits 0. Record warning count without describing warnings as failures.

- [ ] **Step 3: Run all Rust gates fresh**

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: all Rust tests pass and Clippy exits 0.

- [ ] **Step 4: Check the complete diff**

```powershell
git diff --check b0d3fb0..HEAD
git status --short
git diff --stat b0d3fb0..HEAD
```

Expected: no whitespace errors. Confirm that the existing `src-tauri/Cargo.toml` line-ending-only working-tree status was not staged or altered by the behavioral commits.

- [ ] **Step 5: Perform an isolated desktop smoke test**

Launch with isolated build and app-data paths. Verify this exact sequence:

1. Open Quick Access with `Alt+Shift+Space`.
2. Type a record draft, switch to Search, type a query, and switch back using both mouse and `Ctrl+Tab`; both values remain and focus follows the active tab.
3. Click Configure while AI is missing and while auto-enrich is disabled; the visible main window lands on Settings and Quick Access hides.
4. Open Library, edit A, then edit B without closing; the editor shows B and save updates B only.
5. Trigger sensitive metadata rejection with test data; the alert contains no sensitive value.
6. Resize Quick Access to its minimum supported size and repeat the primary actions.
7. Switch to English and repeat configuration and Library error/success paths; affected strings are English.

- [ ] **Step 6: Record verification evidence**

Create `docs/superpowers/verification/2026-07-17-library-quick-access-remediation.md` with command, exit code, test totals, warning totals, isolated paths, and the desktop steps actually observed. Mark any unexecuted item explicitly rather than inferring success.

- [ ] **Step 7: Commit mechanical formatting and verification evidence**

```powershell
git add src-tauri/src
git add -f docs/superpowers/verification/2026-07-17-library-quick-access-remediation.md
git commit -m "verify library remediation"
```

Do not stage `src-tauri/Cargo.toml` unless its content was intentionally changed by an implementation task.
