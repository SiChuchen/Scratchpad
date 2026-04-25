# Soma Scratchpad

**[中文文档](README_ZH.md)**

A lightweight floating dock for Windows that lets you quickly collect and manage text snippets, images, and files.

## Features

### Data Collection

- **Text Paste** — Copy text and Ctrl+V in the dock to create an entry instantly
- **Screenshot Paste** — Take a screenshot and Ctrl+V to save it as an image entry
- **File Paste** — Ctrl+C a file in Explorer, then Ctrl+V to import (doc, xlsx, pdf, etc.)
- **File Drag & Drop** — Drag files from Explorer directly into the dock window

### Content Management

- **Dock** — All collected entries, with expand/collapse and rename support
- **All** — Browse all entries sorted by time
- **Favorites** — Star important entries for separate management
- **Auto Titles** — Text entries get auto-generated summary titles; code content is auto-detected
- **Quick Actions** — Copy content, copy path, favorite, delete

### Minimized Tab

- **Circular Tab** — Minimizes to a 48x48 floating circle that snaps to screen edges
- **Drag & Snap** — Long-press and drag to reposition; snaps to nearest edge on release
- **Full Visibility** — Tab stays entirely within the screen work area
- **Multi-Monitor** — Position calculated based on the current monitor's work area

### Window Controls

- **Transparent Floating** — Borderless, always-on-top semi-transparent window
- **Ctrl+Drag** — Hold Ctrl and drag to move the window
- **Global Hotkey** — Alt+Shift+V to toggle window visibility
- **System Tray** — Runs in background with tray icon control

### Personalization

- **Themes** — 3 built-in presets, plus follow-system mode
- **Fonts** — Separate Chinese/English font settings
- **Window** — Size, position, background color, and transparency
- **Bilingual** — UI language switchable between Chinese and English
- **Auto-Start** — Launch on system startup
- **Auto-Update** — Check for and install new versions

## Tech Stack

- **Frontend**: Svelte 5 + TypeScript + Vite
- **Backend**: Tauri 2 (Rust), SQLite (rusqlite)
- **Platform**: Windows 10+

## Development

```bash
pnpm install
pnpm tauri dev
```

## Build

```bash
pnpm tauri build
```

## Data Storage

Data is stored in a `data/` directory next to the application executable, not on the system drive.

| Path | Content |
|------|---------|
| `<exe_dir>/data/scratchpad.sqlite3` | Entry metadata (text content, collapse state, view membership, etc.) |
| `<exe_dir>/data/assets/YYYY-MM-DD/` | Image and file attachments (organized by date) |

To reset, simply delete the `data/` directory.

## Project Structure

```
src/                          # Svelte frontend
  lib/api/dock.ts             # Tauri IPC call layer
  lib/components/             # UI components
    entry/                    # Entry cards (text, image, file)
    views/                    # Views (dock, all, favorites, settings)
  lib/i18n/                   # Internationalization (locales, types)
  lib/state/                  # Frontend state management
  lib/types/                  # TypeScript type definitions
  App.svelte                  # Main app entry; handles global paste and drag-drop
  MinimizedApp.svelte         # Minimized tab entry

src-tauri/src/                # Rust backend
  models/                     # Data models (entry, preferences)
  scratchpad/                 # Business logic (storage, assets, preferences, clipboard)
  storage/                    # SQLite connection and migrations
  system/                     # System features (tab_controller, window, fonts)
  lib.rs                      # Tauri command registration
```
