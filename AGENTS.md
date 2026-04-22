# Repository Guidelines

## Project Structure & Module Organization
This repository is a Windows-focused Tauri desktop app. The Svelte 5 frontend lives in `src/`: `App.svelte` owns the dock shell, `lib/components/` contains UI cards such as `TextSnippetCard.svelte`, and `app.css` holds shared styling. The Rust backend lives in `src-tauri/src/`: `lib.rs` registers Tauri commands, `models/` defines shared data types, `scratchpad/` contains feature logic, and `storage/` handles SQLite access and migrations. Tauri config is in `src-tauri/tauri.conf.json`. Treat `dist/`, `src-tauri/target/`, and `src-tauri/gen/` as generated output.

## Build, Test, and Development Commands
Use `pnpm` for frontend workflows.

- `pnpm install` installs JS dependencies.
- `pnpm dev` starts the Vite frontend only.
- `pnpm tauri dev` runs the full desktop app with the Rust backend.
- `pnpm build` type-checks and builds the frontend bundle into `dist/`.
- `pnpm tauri build` creates a packaged desktop build.
- `pnpm check` runs `svelte-check` against `tsconfig.json`.
- `cargo test` from `src-tauri/` runs Rust tests.

## Coding Style & Naming Conventions
Follow the existing file style instead of reformatting unrelated code. Svelte and TypeScript files use 2-space indentation, PascalCase component names, and camelCase functions and handlers such as `handleDelete`. Rust follows standard 4-space indentation, `snake_case` function names, and module names that match feature boundaries. Keep Tauri IPC commands prefixed with `ipc_` so frontend `invoke()` calls stay easy to trace.

## Testing Guidelines
Rust tests are supported through `cargo test`; add unit tests close to storage and model code when behavior changes. There is no JS test runner or coverage gate committed yet, so frontend changes should at minimum pass `pnpm check` and be exercised manually in `pnpm tauri dev`. Name tests after observable behavior, for example `toggle_pin_updates_updated_at`.

## Commit & Pull Request Guidelines
No `.git` history is present in this workspace snapshot, so no repository-specific commit pattern can be inferred. Use short imperative commit subjects such as `add tray toggle command` or `refine text snippet editing`. Keep commits scoped to one change area when possible. PRs should include a concise summary, validation steps, linked issues or specs, and screenshots or GIFs for visible UI changes.

## Configuration & Data Handling
Keep the existing `%LOCALAPPDATA%\\Soma\\...` storage convention for SQLite and image files. Avoid hard-coded absolute paths, and do not commit local build artifacts or generated binaries.
