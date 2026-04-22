# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Primary reference

@AGENTS.md contains the authoritative coding guidelines, build commands, style conventions, and testing instructions. Read it first.

## Branching

Feature branches (`feature/*`) merge into `master`, then `master` is merged to `main` for releases.

## Storage architecture

Two SQLite schemas coexist during the transition to the dock model:
- **Legacy**: `scratchpad_items` table — used by IPC commands in `lib.rs`
- **Dock**: `entries` + `home_entries` + `note_entries` tables — the new model in `scratchpad/storage.rs`

When modifying storage code, be aware which schema the code targets. New features should use the dock schema.

## Worktrees

Feature development uses git worktrees under `.worktrees/` (e.g., `.worktrees/scratch-dock` on branch `feature/scratch-dock`). These are gitignored.

## UI language

All user-facing strings are in Simplified Chinese (zh-CN). Do not translate to English.

## Committed build artifacts

`src/main.js` and `src/main.js.map` are compiled from `src/main.ts` and intentionally committed. Do not delete them.
