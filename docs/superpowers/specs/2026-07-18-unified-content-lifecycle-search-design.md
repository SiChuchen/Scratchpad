# Unified Content Lifecycle and Search Design

**Status:** Approved for implementation planning

**Date:** 2026-07-18

**Scope:** Product integration across the original scratchpad workflow and the structured Vault / Quick Access workflow

## 1. Purpose

Soma Scratchpad currently has two capable but partially disconnected product areas:

- The original scratchpad manages temporary text, images, and files through `收纳 / 全部 / 收藏`.
- The newer structured-content system manages credentials, bookmarks, and notes through `资料库`, Quick Access capture, and hybrid search.

The implementation works, but users must understand storage boundaries to know where to create, browse, or search for content. Those boundaries are technical and should not define the product.

This design makes the application one coherent information lifecycle:

1. Users may capture through a lightweight temporary path or an intentional organized path.
2. Every existing item participates in one browse and search model.
3. Every item uses the same temporary-versus-saved lifecycle.
4. Main and Quick Access windows stay synchronized while retaining different interaction densities.

## 2. Current-System Findings

### 2.1 Capabilities already shared successfully

- Both windows use the same theme, locale, font, and preference data.
- Both shortcut paths now use strict visibility toggles.
- Clipboard operations terminate in the same Rust backend.
- Main Vault and Quick Access reuse structured search, detail, and copy primitives.
- All application data lives in the same configured data directory and SQLite database.

These integrations remain in place.

### 2.2 Product and implementation fragmentation

| Area | Current behavior | User-facing problem |
|---|---|---|
| Search | Home, Favorites, Main Vault, and Quick Access own separate search experiences | Users must first guess where an item lives |
| Persistent content | Ordinary favorites and structured Vault entries use separate top-level views | A storage distinction appears as a product distinction |
| `全部` | Includes only original dock entries | It is not actually all content |
| Cross-window updates | Theme changes propagate, but ordinary content mutations do not have one invalidation event | Main and Quick Access may show stale content |
| Capture | Main paste, manual forms, Vault editor, and Quick capture overlap | Retention intent is not explicit or consistent |
| State | Main and Quick Access instantiate separate list, query, notification, and refresh logic | Equivalent operations behave differently |
| Components | `HomeView` and `NoteView` substantially duplicate browsing UI; structured result UIs are separate again | Visual and behavioral drift is likely |
| APIs | `dockApi` and `vaultApi` expose separate product surfaces; deprecated Vault aliases remain | Frontend code follows storage boundaries |

## 3. Product Principles

### 3.1 Two capture paths, one lifecycle

#### Lightweight capture

- Paste text, screenshots, or files.
- Drag files into the main window.
- Add plain content without requesting organization.
- The item enters temporary `收纳` and follows the configured cleanup period.
- Saving / favoriting it moves it into permanent `收藏`.

This preserves the original scratchpad's low-friction local workflow.

#### Organized capture

- Use Quick Access `录入` to parse and organize content.
- Local parsing always runs; AI enrichment is optional.
- Choosing organized capture expresses an intent to keep the result.
- The item therefore enters permanent `收藏` immediately.
- Missing AI configuration, timeout, provider failure, or disabled enrichment does not block saving and does not change retention.

AI enhances organization. It does not decide whether user data survives.

### 3.2 One retention model

Every content type has one of two retention states:

- `temporary`: shown in `收纳`, subject to cleanup.
- `saved`: shown in `收藏`, never subject to automatic cleanup.

Transitions are consistent across content types:

- Save / favorite: `temporary -> saved`.
- Unsave / unfavorite: `saved -> temporary`, with cleanup timing starting at that transition.
- Delete: explicit removal with a short undo opportunity.

Existing Vault entries migrate as `saved`. Existing ordinary favorites remain `saved`.

### 3.3 One search mental model

Search includes every currently existing item:

- temporary text, images, and files;
- saved text, images, and files;
- credentials, bookmarks, and structured notes;
- locally organized and AI-enriched content.

Results expose content type and retention state, not storage source or whether AI contributed metadata.

### 3.4 One dataset, two interaction densities

- Quick Access optimizes for finding and immediately using content: copy, reveal, open, and paste elsewhere.
- The main window optimizes for managing content: inspect, edit, save, unsave, reorder, and delete.

Both surfaces share query semantics, ranking, result models, lifecycle rules, and change events. They keep independent query, selection, and capture-draft state so one window never disrupts work in the other.

## 4. Target Information Architecture

The main navigation becomes:

1. `收纳`: all temporary content.
2. `全部`: all currently existing content.
3. `收藏`: all saved content, including former ordinary favorites and former Vault entries.
4. `设置`: application-wide configuration.

`资料库` is removed as a top-level product concept. Credential, bookmark, note, text, image, and file remain content-type filters within `全部`, `收藏`, and search.

Quick Access keeps its two primary modes:

- `录入`
- `搜索`

The floating main-window button remains an open / focus action for Quick Access.

## 5. Unified Domain Contract

The frontend must stop using a storage type as its primary cross-product model. A unified application contract sits above the original and Vault repositories.

### 5.1 Identity

Every item receives an opaque, namespaced content ID:

```text
dock:<existing-dock-id>
vault:<existing-vault-id>
```

The frontend treats the value as opaque. Namespacing prevents collisions without rewriting existing primary keys.

### 5.2 Summary model

The unified summary contains only information required to browse and search:

```text
UnifiedContentSummary
  id
  kind: text | image | file | credential | bookmark | note
  retention: temporary | saved
  title
  preview
  createdAt
  updatedAt
  cleanupAt?
  capabilities
```

`capabilities` declares supported actions such as `copyText`, `copyImage`, `copyFile`, `copyPath`, `openUrl`, `revealSensitive`, `edit`, `save`, `unsave`, and `delete`. UI components render actions from capabilities rather than checking the storage source.

### 5.3 Detail model

Details are a tagged union by content kind:

- Text and note: editable body and title.
- Image: preview metadata, asset URL, copy-image and copy-path actions.
- File: filename, size, path, copy-file and copy-path actions.
- Credential: ordered structured fields and sensitivity flags.
- Bookmark: URL and supporting metadata.

Existing type-specific detail components may be adapted behind a shared result shell. The design does not force dissimilar payloads into one oversized component.

## 6. Unified Catalog and Existing Storage

### 6.1 Storage strategy

Implementation uses a unified read model over the two existing content stores. It does not perform a full content migration.

Existing payload ownership remains:

- Original entries, memberships, and attachment paths remain in the original scratchpad tables.
- Credentials, bookmarks, notes, fields, tags, and AI metadata remain in Vault tables.

A lightweight `content_catalog` records only cross-domain metadata:

```text
content_catalog
  unified_id          primary key
  source              dock | vault
  source_id           original primary key
  kind
  retention_state     temporary | saved
  retention_changed_at
  cleanup_at?
  inbox_position?
  saved_position?
  created_at
  updated_at
```

`(source, source_id)` is unique. Full bodies, asset bytes, secrets, and sensitive field values are not duplicated into the catalog.

### 6.2 Authority and transactions

The unified content service becomes the authority for cross-domain lifecycle metadata. Type-specific repositories remain authoritative for payloads.

Every create, update, retention transition, reorder, or delete updates payload storage and catalog state in one SQLite transaction. A catalog failure rolls back the complete mutation; the application must never commit a payload change while leaving the unified view stale. Once the catalog exists, legacy IPC commands must delegate through this service or a repository hook that performs the same atomic catalog update; no old write path may bypass it.

### 6.3 Upgrade and backfill

The migration is transactional and idempotent:

- One catalog row is created per distinct original entry, even if it has both Home and Note memberships.
- An original entry with a Note / favorite membership becomes `saved`.
- An original entry without a favorite membership becomes `temporary`.
- Every existing Vault entry becomes `saved`.
- Existing Home and Favorites ordering is retained.
- Existing Vault entries are appended to the saved sequence by descending update time.
- Original attachment paths and payload rows are unchanged.

Re-running the backfill updates missing catalog rows without duplicating existing mappings.

## 7. Unified Search

### 7.1 Search projection

A unified FTS projection indexes safe, useful text:

- text body and title;
- image title and filename;
- file title and filename;
- credential title, tags, and explicitly non-sensitive fields;
- bookmark title, URL, notes, and tags;
- structured-note title, body, and tags;
- approved AI summary and aliases that already passed current sensitive-metadata validation.

Structured fields explicitly marked sensitive, including passwords, tokens, and private keys, never enter the catalog, FTS projection, logs, result previews, or AI context. Original free-form text remains locally searchable because it has no field-level sensitivity metadata and was already searchable in the original workflow. Free-form content is never sent to the AI provider as part of search planning.

### 7.2 Query behavior

1. A query immediately executes against the unified local projection.
2. If hybrid search is configured and enabled, the existing safe query-planning flow may generate an AI plan.
3. The plan refines the same local unified search; no content catalog is sent to the provider.
4. AI failure leaves the local result set usable and shows a non-blocking fallback status.

Default ordering is:

1. relevance score;
2. updated time descending for equal relevance;
3. stable unified ID tie-breaker.

Type filters operate on content kind, never on source storage.

### 7.3 Browse ordering

- `收纳`: manual unified order; new temporary items enter at the top.
- `收藏`: manual unified order; organized captures enter at the top after creation.
- `全部`: updated time descending; manual reorder is disabled.
- Search: relevance order; manual reorder is disabled.

## 8. Application Services and Events

### 8.1 Rust service boundary

A new content orchestration module owns unified operations and delegates payload work to the existing scratchpad and Vault repositories.

The frontend-facing command surface provides:

- list by retention scope and optional content kind;
- unified search;
- load detail;
- save / unsave;
- reorder temporary or saved content;
- delete and restore during the undo window;
- content revision lookup.

Type-specific copy and open operations continue using capability-specific backend commands.

### 8.2 Content revision and events

Each committed mutation increments a monotonic content revision and emits one event after commit:

```text
content-changed
  revision
  changes[]
    id
    operation: created | updated | retention | reordered | deleted | restored
```

Main and Quick Access subscribe to this event.

- An active query is rerun without clearing query text.
- A selected item remains selected if it still exists.
- Updated selected details are reloaded.
- A remotely deleted selection is cleared with an explicit notice.
- Capture drafts are never reset by a content event.

When either window is shown or focused, it compares its last revision with the backend revision. A mismatch triggers refresh, covering missed events and suspended webviews.

AI metadata updates flow through the same unified invalidation path after their database commit.

## 9. Frontend Consolidation

### 9.1 Shared controllers

- `UnifiedContentController` owns browse scope, type filter, ordering, lifecycle mutations, and undo state.
- `UnifiedSearchController` replaces the separate Home, Favorites, Vault, and Quick search behavior.
- Main and Quick Access instantiate separate controller instances backed by the same API. UI session state remains isolated.

### 9.2 Shared components

- `UnifiedSearchInput`: common query behavior and status presentation.
- `UnifiedResultList`: common result ordering, selection, and keyboard navigation.
- `UnifiedItemCard`: common title, preview, kind, retention, and capability-driven action shell.
- Type-specific detail bodies remain focused components.
- A shared notification-message layer defines copy, lifecycle, refresh, and error text while each window retains an appropriate visual presentation.

### 9.3 Views to merge

- `HomeView` and `NoteView` converge into one retention-scoped browse view instead of maintaining duplicate forms, search boxes, drag logic, and list rendering.
- `CategoriesView` becomes the `全部` scope with content-kind filters.
- `VaultView` no longer exists as a top-level view; its structured editor and detail bodies are invoked from unified cards and results.
- Quick `SearchMode` switches to unified search and unified IDs while retaining its action-first two-pane layout.

The main search input is persistent below navigation. When query text is non-empty, the content body shows global results regardless of the selected browse scope. Clearing the query or pressing Escape restores the previous browse view and position.

## 10. Lifecycle Operations

### 10.1 Save

- Original temporary entries gain saved catalog state and maintain their existing payload and attachment identity.
- Structured temporary entries gain saved catalog state without rewriting fields or tags.
- Cleanup scheduling is removed.

### 10.2 Unsave

- The catalog state becomes temporary.
- The item enters the temporary order at the top.
- Cleanup timing begins from the transition, not from original creation time.
- Existing payload data is preserved.

### 10.3 Cleanup

The configured cleanup period applies to all `temporary` catalog rows. Saved rows are excluded by invariant. Cleanup deletes through the unified service so payload storage, attachments, catalog rows, FTS rows, revisions, and events remain consistent.

### 10.4 Delete and undo

All content types use one visible undo pattern. The initial action removes the item optimistically from UI and records the restore information required by its adapter. If the undo interval expires, deletion commits through the unified service. Commit failure restores the item and reports an error.

## 11. UI Behavior

### 11.1 Result identity

Cards show:

- content icon and type;
- title and safe preview;
- `临时` or `收藏` retention state;
- cleanup timing for temporary content when useful;
- capability-appropriate actions.

Cards do not show `旧版`, `Vault`, `AI 来源`, or internal source labels.

### 11.2 Quick Access

Quick results provide only safe, non-destructive actions:

- copy text, fields, image, file, or path;
- reveal and re-mask sensitive fields;
- open bookmarks;
- open the corresponding main-window item for editing or management.

Deletion, retention changes, and complex editing remain main-window actions.

### 11.3 Main window

Main results may provide:

- details and editing;
- save / unsave;
- delete with undo;
- type-specific copy and open actions;
- drag ordering in `收纳` and `收藏` only.

## 12. Error Handling and Recovery

- Payload and catalog mutations are atomic.
- Migration failure rolls back the entire schema/backfill transaction.
- Local search remains available when AI configuration or requests fail.
- Search refresh errors retain the last valid result set and current query.
- A missing attachment shows an unavailable state but leaves the item editable and deletable.
- Missed content events are repaired by revision comparison on focus/show.
- A deleted remote selection clears safely; it never leaves sensitive detail visible.
- Logs and user-visible errors identify item IDs and operation types without including secret values.

## 13. Testing Strategy

### 13.1 Rust

- idempotent catalog migration from realistic old databases;
- identity namespacing and collision handling;
- retention transitions and cleanup timing;
- atomic payload/catalog/FTS updates and rollback;
- original order preservation and unified reorder;
- mixed local search ranking;
- explicitly sensitive structured values absent from catalog and FTS;
- revision increment and event payload generation;
- delete, undo, restore, and attachment cleanup.

### 13.2 Frontend

- shared result rendering for all six content kinds;
- capability-driven actions;
- global search from every main browse scope;
- clearing search restores prior scope and scroll state;
- Quick and main controllers retain independent query state;
- content events refresh active results without clearing drafts;
- selection behavior for update and remote deletion;
- lifecycle labels, cleanup hints, filters, and keyboard navigation;
- mixed-content drag ordering only in allowed scopes.

### 13.3 Windows runtime

- original text, image, file paste, drag, edit, copy, and cleanup behavior;
- organized capture with AI enabled, disabled, unavailable, and failing;
- save and unsave across original and structured content;
- cross-window create, edit, retention, and delete synchronization;
- copy behavior for text, images, files, paths, bookmarks, and sensitive fields;
- theme, locale, shortcuts, focus persistence, pinning, and minimum window sizes;
- upgrade using a disposable copy of a representative existing database.

## 14. Delivery Sequence

1. Add unified contracts, catalog migration, adapters, revision, and events behind tests.
2. Build the unified local projection and search command; preserve current Vault search as a compatibility path until both clients migrate.
3. Add shared frontend controllers and result components.
4. Consolidate the main navigation and retention-scoped views.
5. Move Quick Access search to the unified API and add main-window deep links.
6. Unify lifecycle, cleanup, delete/undo, and reorder operations.
7. Remove obsolete view-local searches, dead event listeners, and deprecated Vault aliases after call-site verification.
8. Update README screenshots, product terminology, data behavior, and verification records.

Each phase must keep the application runnable and must not mutate real user data during tests.

## 15. Non-Goals

- Full migration into one payload table.
- Moving or rewriting existing attachments.
- Cloud synchronization or multi-device conflict resolution.
- OCR, embeddings, or a vector database.
- Sending the content catalog to an AI provider.
- Sharing in-progress query text, selection, or capture drafts between windows.
- Redesigning theme presets or shortcut semantics.

## 16. Acceptance Criteria

The integration is complete when:

1. Users can browse `收纳`, `全部`, and `收藏` without a separate `资料库` concept.
2. One search finds every currently existing content type from either window.
3. Quick and main results use the same IDs, ranking, lifecycle, and safe previews.
4. Lightweight capture defaults to temporary; organized capture defaults to saved even without AI.
5. Every content type can transition between temporary and saved without payload loss.
6. Temporary cleanup never deletes saved content.
7. A mutation in one window updates the other without losing query or draft state.
8. Existing content and attachments survive the upgrade, and existing browse order is retained.
9. Explicitly sensitive structured values never enter unified metadata, search indexes, logs, or AI context; free-form content remains local-only during search.
10. Automated and Windows runtime verification pass with no new errors or changed-file warnings.
