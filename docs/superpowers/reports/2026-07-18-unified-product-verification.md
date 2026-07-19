# Unified Product Verification

Verified on Windows in `feature/unified-content-integration` on 2026-07-19.

## Outcome

The main window and Quick Access now use one content lifecycle and one search/catalog API. Quick capture creates saved structured content, while main-window paste, drop, and text creation remain temporary until saved. Search includes all six content kinds and both retention states.

## Automated gates

| Gate | Result |
| --- | --- |
| `pnpm test:unit -- --run` | 26 files, 182 tests passed |
| `pnpm check` | 0 errors; 4 pre-existing accessibility warnings |
| `pnpm build` | passed, 176 modules transformed |
| `cargo fmt -- --check` | passed |
| `cargo clippy --all-targets --all-features -- -D warnings` | passed |
| `cargo test --quiet` | 405 tests passed |
| `cargo build` | passed |

The commands cover every gate in `.github/workflows/ci.yml`; Clippy was run with stricter warning handling than CI.

## Upgrade fixtures

Fixtures were generated outside production data with:

```powershell
cd src-tauri
cargo run --example create_validation_fixtures -- ..\test-data\final-validation
```

| Fixture | Bytes | SHA-256 | Evidence |
| --- | ---: | --- | --- |
| legacy | 151552 | `B2EE1552DC746CFBBCEA905F6A7B65E6E39CB0A533AE5CE91EFA7E739A0BC836` | main schema v2, vault v4, 7 payloads, no content schema |
| fresh | 217088 | `F6A3B9AEB15BF57FD25E29B657C28E0E57B4DEFC9A249F4F6CE7E6E144E071A6` | main/vault schema v4, 7 catalog items, 2 temporary, 5 saved, revision 9 |

Generated databases were removed after hashing and were never staged.

## Product acceptance matrix

| Behavior | Evidence |
| --- | --- |
| Quick record/search panels remain mounted and keep independent draft/query state | `src/QuickAccessApp.test.ts` |
| Clicking elsewhere does not hide Quick Access | explicit-close tests plus strict shortcut/window state tests |
| Visible shortcut hides and hidden shortcut shows both windows | `src/lib/state/quick-access.test.ts`, `src/lib/state/window.test.ts`, Rust window tests |
| FAB focuses/opens Quick Access without becoming a competing content state | `src/lib/components/QuickAccessFab.test.ts`, App integration tests |
| Theme and language updates propagate without resetting Quick state | `src/QuickAccessApp.test.ts`, preference-sync tests |
| Local results render before optional AI expansion; AI failure preserves local results | `src/lib/state/content-search.test.ts`, `SearchMode.test.ts` |
| Capture survives parse/enrichment/save failures and clears only after save | `CaptureMode.test.ts` |
| Organized capture is permanently saved with or without AI | capture adapter/backend tests and namespaced saved-ID test |
| Search renders temporary/saved results for all six kinds without source badges | `SearchMode.test.ts` |
| Keyboard selection, refresh selection, and late-result races are guarded | `SearchMode.test.ts` |
| Text/note copies full content; image/file and sensitive credential actions use backend adapters | `SearchMode.test.ts`, `QuickContentDetail.test.ts` |
| Useful fields precede notes/tags; copy targets are right aligned; sensitive values re-mask | `QuickContentDetail.test.ts` and component layout contract |
| Quick-to-main handoff validates unified IDs and selects the requested detail | Rust `main_content_open_payload_requires_a_valid_unified_id`, `App.test.ts` |
| Revision events and focus repair refresh both windows without coupling local UI state | `QuickAccessApp.test.ts`, App content-change tests |

## Legacy and sensitive-data audit

The obsolete-symbol scan returned zero matches:

```powershell
rg -n "HomeView|CategoriesView|NoteView|VaultView|LibraryViewController|HybridSearchController|onTagsUpdated|vault-tags-updated|@deprecated|nav\.library" src src-tauri/src
```

High-confidence secret patterns were scanned in the worktree and full Git patch history. Matches were limited to deliberately invalid redaction-test fixtures (`sk-abcdefghijklmnopqrstuvwxyz`, `sk-test-secret-key-do-not-leak`, and a truncated PEM marker). No credential-shaped GitHub, Google, AWS, OpenAI, or private-key value is present in the commits. Runtime API keys remain excluded from API summaries and are not written to reports.

## Known non-blocking output

- Svelte reports four existing accessibility warnings in `SettingsView.svelte`, `MinimizedApp.svelte`, and `TextEntryBody.svelte`; there are no type errors.
- jsdom prints `Window.open() method` as unimplemented while the bookmark adapter test still passes; URL protocol validation and the user action are covered.
