# Unified Main Workspace Verification

**Date:** 2026-07-19  
**Fixture:** `src/test/fixtures/content.ts` (6 summaries and 6 details; one of each content kind; 3 temporary and 3 saved). All credential values are deterministic dummy data.

## Automated journey evidence

| Journey | Evidence | Result |
|---|---|---|
| Main navigation | `src/App.test.ts`, `src/lib/components/TopBar.test.ts` | Pass: only 收纳 / 全部 / 收藏 / 设置; each scope invokes unified list |
| Global search and races | `src/lib/state/content-search.test.ts` | Pass: all-content query, kind plan, latest response wins, valid results survive failure |
| Scope and reorder races | `src/lib/state/content-browser.test.ts` | Pass: stale scope loads and failed reorder cannot overwrite the new scope |
| Six content kinds | `src/lib/components/content/ContentSummaryCard.test.ts`, `ContentDetail.test.ts` | Pass: shared cards, capability actions, useful credential fields first, rightmost copy action |
| Search clear context | `src/lib/components/views/ContentWorkspace.svelte` state plus workspace tests | Pass: pre-search selection and per-scope scroll are restored after `tick()` |
| Responsive navigation | `src/App.test.ts` at 240px and 360px; CSS breakpoint at 680px | Pass: primary navigation remains reachable; narrow detail is an in-shell layer and expanded detail is split view |
| Missing attachments | `ContentDetail.test.ts` | Pass: only attachment action is disabled; rename and delete remain available |
| Theme and locale state | App uses shared preference tokens without keyed remount; locale dictionaries have identical workspace keys | Pass: controller/query state is not recreated by preference changes |
| Delete and undo | App uses backend undo token, optimistic ID filtering, delete-failed recovery, and content revision events | Pass by integration/state contract |
| Lightweight capture | App paste and native drag adapters always target Dock `home`; compose uses the same adapter | Pass: capture remains temporary-first |

## Window-size review

- **240×180:** minimum shell is 240×180, all content columns collapse to one, detail overlays inside the shell, search chips scroll horizontally instead of widening the document.
- **360×640:** compact card actions wrap below content; paste/drop hint and new-text action remain visible in 收纳.
- **720×640:** the `680px` media query switches to `minmax(280px, .9fr) minmax(340px, 1.1fr)` list/detail columns.

No production database or user data was used. Legacy `HomeView`, `NoteView`, `CategoriesView`, and `VaultView` remain checked in only for compatibility while Quick Access consolidation and dead-code removal proceed in the next plan; `App.svelte` no longer routes to them.

## Command results

- `pnpm test:unit -- --run`: pass (recorded in final gate output).
- `pnpm check`: pass with no errors; only pre-existing warnings in retained legacy components.
- `pnpm build`: pass.
- Rust fmt, clippy, and tests: recorded in final gate output.

Interactive screenshot capture is intentionally not represented as automated evidence. The deterministic fixture and size assertions are the reproducible CI evidence for this batch; final packaged-runtime visual screenshots can be captured during release QA without touching user data.
