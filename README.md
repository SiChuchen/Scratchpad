# Soma Scratchpad

Soma Scratchpad is a Windows desktop workspace for collecting temporary material and turning useful information into a searchable local library. It is designed for AI-assisted work without forcing screenshots, copied files, notes, credentials, or paths into a project repository.

[Download the latest release](https://github.com/SiChuchen/Scratchpad/releases/latest) · [Chinese README](README_ZH.md) · [MIT License](LICENSE)

## What it does

### Unified content workspace

- Paste text, screenshots, and copied files into a desktop staging area.
- Drag files into the window and create editable text entries directly.
- Browse **Dock** (temporary), **Saved** (persistent), and **All** content from one workspace.
- Search text, images, files, credentials, bookmarks, and notes together; filter by content type.
- Reorder items where supported, rename entries, edit text and structured notes, and undo supported deletions.
- Open a file or image in its containing folder, copy its content, or copy its path for use in terminals and AI tools.
- Export everything to **Excel (.xlsx)**, **CSV**, **Markdown**, or **JSON** from Settings for backup and migration; sensitive fields are masked as `******` by default, with an opt-in to include them.

### Temporary staging without repository pollution

- New pasted or dropped content enters the temporary Dock by default.
- Save/favorite an item to retain it permanently; restore it to temporary retention when no longer needed.
- Configure automatic cleanup for temporary material.
- Images and imported files are copied into the application's data directory rather than a working repository.

### Global Quick Access

- Open a focused capture and search window from a configurable global shortcut (default: `Alt+Shift+Space`).
- Record mode performs immediate local parsing, then optionally enriches the draft with AI.
- Capture text is saved as a structured **credential**, **bookmark**, or **note**. Every field remains editable before saving.
- Search mode searches the unified workspace, previews a selected result, copies values or paths, and opens the main window for management.
- `Ctrl+Tab` switches between Record and Search; `Ctrl+Enter` saves a capture; `Escape` hides the Quick Access window.

### Local structured library

The library is suitable for operational notes, runbooks, service endpoints, connection details, bookmarks, and credentials.

- Fields have explicit sensitive/non-sensitive flags.
- Sensitive values are hidden in the UI when appropriate, can be cleared from the clipboard after a configurable delay, and are excluded from searchable projections.
- Local parsing recognizes connection URLs, SSH commands, user/password host strings, single URLs, multi-line key-value material, and mixed Chinese/English deployment documents.
- If AI is unavailable, the local draft remains editable and saveable.

### Optional AI organization and search

AI is optional. Configure a compatible provider and model in Settings; built-in presets include DeepSeek, OpenAI, Kimi, Zhipu, Qwen, OpenRouter, and custom OpenAI-compatible endpoints.

- Automatic capture enrichment creates a title, type, fields, tags, summary, and search aliases.
- Structured extraction preserves explicit URLs, paths, IPs, ports, versions, emails, and sensitive placeholders instead of collapsing them into prose.
- Automatic hybrid search can ask AI to expand a query plan while local search remains the fallback.
- DeepSeek thinking mode is a separate setting and is **off by default** for predictable structured output. It can be enabled later when a task benefits from reasoning.
- Truncated responses are retried once with a larger output budget. Authentication, rate-limit, timeout, network, and truncation failures are surfaced without blocking local capture.

#### AI data handling

- The API key is stored locally in the application configuration. It is not sent to the frontend, but it is not encrypted at rest.
- Before a capture is sent to an AI provider, known sensitive values are replaced with request-local `[SECRET:...]` placeholders.
- The application rejects unknown placeholders and prevents sensitive values from being written into AI tags, summaries, or aliases.
- Enabling AI sends the masked capture text or masked search query to the configured external provider. Do not enable it for data that must never leave the device.

### Desktop behavior and settings

- Always-on-top pin mode, edge minimization, system-tray controls, and optional startup on login.
- Separate configurable global shortcuts for the main window and Quick Access.
- Dark Glass, Light Matte, Light Frosted, and system theme behavior.
- Independent Chinese and English font settings, instant UI language switching, update proxy configuration, configurable data directory, and automatic updates.

## Install

Download an asset from [GitHub Releases](https://github.com/SiChuchen/Scratchpad/releases/latest):

| Asset | Use |
| --- | --- |
| `Soma_Scratchpad_x.y.z_Windows.exe` | NSIS installer; recommended for most users |
| `Soma_Scratchpad_x.y.z_Windows.msi` | MSI installer |
| `Soma_Scratchpad_x.y.z_Windows_Portable.zip` | Portable build; extract and run |

Windows 10 or later is required. The updater verifies signed update metadata from the release channel.

## Data location and backup

By default, application data lives in a `data/` directory beside the executable:

```text
<application directory>/
  soma-scratchpad.exe
  data/
    scratchpad.sqlite3
    assets/
```

Back up the `data/` directory to preserve entries and attachments. Changing the data directory does not automatically migrate existing data.

## Development

Prerequisites: Node.js, pnpm, Rust stable, and the Windows prerequisites required by Tauri 2.

```bash
pnpm install
pnpm tauri dev
```

Useful checks:

```bash
pnpm check
pnpm test:unit
pnpm build

cd src-tauri
cargo test
cargo fmt --check
cargo clippy
```

Create installers and updater artifacts:

```bash
pnpm tauri build
```

The build output is written below `src-tauri/target/release/bundle/`.

## Release process

The release workflow runs when a `v*` tag is pushed. It builds signed Windows installers, a portable archive, and `latest.json`, then publishes them as a GitHub Release.

Before releasing, run the checks above. AI organization regression can additionally be exercised with:

```powershell
.\scripts\Invoke-AiOrganizationEvaluation.ps1 -DatabasePath <path-to-scratchpad.sqlite3>
```

The evaluation script reads an already saved local configuration at runtime. It does not contain an API key or upload one to the repository.

## Technology

| Layer | Technology |
| --- | --- |
| Desktop runtime | Tauri 2 |
| Backend | Rust |
| Frontend | Svelte 5, TypeScript, Vite |
| Local storage | SQLite |
| Platform | Windows 10+ |

## License

[MIT](LICENSE)
