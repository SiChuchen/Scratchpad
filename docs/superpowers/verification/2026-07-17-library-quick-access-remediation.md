# Library and Quick Access Remediation Verification

Date: 2026-07-17

## Automated gates

- `pnpm test:unit`: passed, 15 files and 108 tests.
- `pnpm check`: passed with 0 errors. Seven pre-existing accessibility warnings remain outside the remediated Library and Quick Access flows.
- `pnpm build`: passed.
- `cargo test` in `src-tauri/`: passed, 239 tests.
- `cargo clippy --all-targets -- -D warnings` in `src-tauri/`: passed.
- `cargo fmt -- --check` in `src-tauri/`: passed.
- `pnpm tauri build --no-bundle`: passed and produced the release executable.
- `git diff --check`: passed.

## Desktop smoke test

The release executable was launched against a generated portable data directory under `src-tauri/target/release/data`; no user database was used. WebView accessibility was enabled for deterministic UI Automation inspection.

Verified behaviors:

- Library `Open quick access` opens the Quick Access window.
- Record and Search are exposed as tabs with persistent tab panels.
- A record draft remains intact after switching to Search and back.
- A search query remains intact after switching modes and hiding/showing the window.
- The active input is focused after mode switching in the component regression test.
- The unavailable-AI banner exposes an actionable `Configure now` button.
- `Configure now` hides Quick Access, shows the main window, and opens Settings.
- The main Settings view and Quick Access both render the persisted English locale.
- The main Settings route remains usable after returning from Quick Access.

Desktop screenshots and transient runtime data are kept under `.codex-run/remediation/` and generated target directories; they are intentionally not committed.

## Security regression

- Sensitive metadata rejection returns the stable `sensitive_metadata_rejected` code without including the rejected value.
- Common credential aliases merge into canonical username/password fields without overwriting the user's displayed key.
- Editing entry B after entry A mounts B's data and saves to B's ID.
