# Unified Content Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在不改变现有主窗口和快捷窗口交互的前提下，为旧收纳条目与结构化资料建立统一身份、生命周期、检索、排序和跨窗口变更协议。

**Architecture:** 保留 `entries`/`home_entries`/`note_entries` 与 `vault_*` 两套载荷存储，在同一个 SQLite 数据库上增加 `content_catalog`、`content_fts` 和单调递增的内容修订号。现有写路径在原事务内同步目录与检索投影；新增 `content::service` 作为后续界面的唯一读模型和生命周期入口，Tauri 事件只负责快速失效，修订号负责恢复漏失事件。

**Tech Stack:** Rust, rusqlite, SQLite FTS5, Tauri 2 events/IPC, TypeScript, Vitest

---

## Prerequisites and delivery boundary

- Read the approved design first: `docs/superpowers/specs/2026-07-18-unified-content-lifecycle-search-design.md`.
- Execute this plan before `2026-07-18-unified-main-workspace.md` and `2026-07-18-unified-quick-access-sync.md`.
- Keep `src-tauri/Cargo.toml` out of every commit; its current line-ending-only change predates this work.
- This plan must leave the current Home/Categories/Note/Vault/Quick Access screens operational. It adds the unified contract but does not switch those screens yet.
- Every database mutation must update payload, `content_catalog`, `content_fts`, and `content_state.revision` in one SQLite transaction.
- Explicitly sensitive structured fields never enter `content_fts`, logs, or an AI search request. Existing free-form Dock text remains locally searchable.

## File map

### New Rust files

- `src-tauri/src/content/mod.rs` — content module exports.
- `src-tauri/src/content/models.rs` — IPC-safe domain types, opaque IDs, scopes, capabilities, mutations.
- `src-tauri/src/content/migrations.rs` — main-schema migration 3 and idempotent legacy backfill.
- `src-tauri/src/content/catalog.rs` — catalog rows, revision bumping, source adapters, order allocation.
- `src-tauri/src/content/projection.rs` — safe local-search document generation and FTS synchronization.
- `src-tauri/src/content/search.rs` — browse listing and deterministic unified local ranking.
- `src-tauri/src/content/service.rs` — list/detail/lifecycle/reorder/delete/restore/cleanup application service.
- `src-tauri/src/content/ipc.rs` — Tauri commands and `content-changed` emission helpers.

### New frontend files

- `src/lib/types/content.ts` — TypeScript mirror of the Rust unified contract.
- `src/lib/api/content.ts` — typed unified IPC and event wrapper.
- `src/lib/api/content.test.ts` — command names, arguments, and event payload contract.

### Existing files changed in this plan

- `src-tauri/src/lib.rs` — module registration, initialization order, commands, and event emission.
- `src-tauri/src/scratchpad/storage.rs` — transactional catalog/projection hooks and removal of startup-only Home cleanup.
- `src-tauri/src/scratchpad/assets.rs` — propagate unified mutations for image/file imports.
- `src-tauri/src/vault/storage.rs` — transactional catalog/projection hooks for all structured writes.
- `src-tauri/src/vault/jobs.rs` — refresh unified projection and emit after AI metadata changes.
- `src-tauri/src/vault/ipc/entries.rs` — emit content mutations after successful structured CRUD.
- `src-tauri/src/vault/ipc/capture.rs` — emit saved-content creation after organized capture.
- `src-tauri/src/vault/ipc/search.rs` — keep existing Vault search compatible while accepting the shared query-plan adapter.

## Contract locked by this plan

```rust
pub enum ContentSource { Dock, Vault }
pub enum ContentKind { Text, Image, File, Credential, Bookmark, Note }
pub enum RetentionState { Temporary, Saved }
pub enum BrowseScope { Temporary, All, Saved }
pub enum ContentOperation { Created, Updated, Retention, Reordered, Deleted, Restored }

pub struct UnifiedContentId(String); // "dock:<source_id>" or "vault:<source_id>"

pub struct ContentSummary {
    pub id: String,
    pub kind: ContentKind,
    pub retention: RetentionState,
    pub title: String,
    pub preview: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub cleanup_at: Option<String>,
    pub capabilities: ContentCapabilities,
}
```

The Rust and TypeScript names above are stable across all three plans.

`ContentDetail` is tagged by user-facing kind, never by storage source:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ContentDetail {
    Text {
        summary: ContentSummary,
        title: String,
        body: String,
    },
    Image {
        summary: ContentSummary,
        file_name: String,
        asset_path: String,
        mime_type: Option<String>,
        width: Option<i64>,
        height: Option<i64>,
        available: bool,
    },
    File {
        summary: ContentSummary,
        file_name: String,
        asset_path: String,
        mime_type: Option<String>,
        size_bytes: Option<i64>,
        available: bool,
    },
    Credential {
        summary: ContentSummary,
        fields: Vec<UnifiedField>,
        notes: Option<String>,
        tags: Vec<UnifiedTag>,
    },
    Bookmark {
        summary: ContentSummary,
        url: String,
        fields: Vec<UnifiedField>,
        notes: Option<String>,
        tags: Vec<UnifiedTag>,
    },
    Note {
        summary: ContentSummary,
        body: String,
        fields: Vec<UnifiedField>,
        tags: Vec<UnifiedTag>,
    },
}
```

Every variant carries only the opaque unified ID inside `summary`. No detail payload exposes `source`, `source_id`, table names, or a value that requires the frontend to strip an ID prefix.

### Task 1: Define unified identity and serialized domain types

**Files:**
- Create: `src-tauri/src/content/mod.rs`
- Create: `src-tauri/src/content/models.rs`
- Modify: `src-tauri/src/lib.rs:1-5`
- Test: `src-tauri/src/content/models.rs`

- [ ] **Step 1: Write failing identity and serialization tests**

Add the following test module at the bottom of the new `models.rs` before defining the referenced types:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_id_round_trips_both_sources() {
        let dock = UnifiedContentId::new(ContentSource::Dock, "de-17");
        let vault = UnifiedContentId::new(ContentSource::Vault, "ve-9");

        assert_eq!(dock.as_str(), "dock:de-17");
        assert_eq!(vault.as_str(), "vault:ve-9");
        assert_eq!(UnifiedContentId::parse(dock.as_str()).unwrap(), dock);
        assert_eq!(UnifiedContentId::parse(vault.as_str()).unwrap(), vault);
    }

    #[test]
    fn opaque_id_rejects_missing_or_unknown_namespace() {
        assert!(UnifiedContentId::parse("de-17").is_err());
        assert!(UnifiedContentId::parse("other:17").is_err());
        assert!(UnifiedContentId::parse("dock:").is_err());
    }

    #[test]
    fn capabilities_are_kind_specific() {
        let file = ContentCapabilities::for_item(
            ContentKind::File,
            RetentionState::Temporary,
            true,
        );
        let credential = ContentCapabilities::for_item(
            ContentKind::Credential,
            RetentionState::Saved,
            false,
        );

        assert!(file.copy_file);
        assert!(file.copy_path);
        assert!(file.save);
        assert!(file.reorder);
        assert!(!file.reveal_sensitive);
        assert!(credential.copy_text);
        assert!(credential.reveal_sensitive);
        assert!(credential.unsave);
        assert!(!credential.save);
    }
}
```

- [ ] **Step 2: Run the focused test and verify the red state**

Run from `src-tauri/`:

```powershell
cargo test content::models::tests -- --nocapture
```

Expected: compilation fails because `UnifiedContentId`, `ContentSource`, `ContentKind`, and `ContentCapabilities` are not defined.

- [ ] **Step 3: Add the complete domain contract**

Define the enums with `Serialize`/`Deserialize` and `#[serde(rename_all = "camelCase")]`, except source/kind/retention values which use lowercase. Implement:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnifiedContentId(String);

impl UnifiedContentId {
    pub fn new(source: ContentSource, source_id: &str) -> Self {
        Self(format!("{}:{source_id}", source.as_str()))
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let (namespace, source_id) = value
            .split_once(':')
            .ok_or_else(|| "content id must contain a namespace".to_string())?;
        ContentSource::parse(namespace)
            .ok_or_else(|| format!("unknown content namespace: {namespace}"))?;
        if source_id.is_empty() {
            return Err("content source id cannot be empty".to_string());
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentSource {
    Dock,
    Vault,
}

impl ContentSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dock => "dock",
            Self::Vault => "vault",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dock" => Some(Self::Dock),
            "vault" => Some(Self::Vault),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Text,
    Image,
    File,
    Credential,
    Bookmark,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RetentionState {
    Temporary,
    Saved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowseScope {
    Temporary,
    All,
    Saved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentOperation {
    Created,
    Updated,
    Retention,
    Reordered,
    Deleted,
    Restored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentCapabilities {
    pub copy_text: bool,
    pub copy_image: bool,
    pub copy_file: bool,
    pub copy_path: bool,
    pub open_url: bool,
    pub reveal_sensitive: bool,
    pub edit: bool,
    pub save: bool,
    pub unsave: bool,
    pub delete: bool,
    pub reorder: bool,
}

impl ContentCapabilities {
    pub fn for_item(
        kind: ContentKind,
        retention: RetentionState,
        reorderable: bool,
    ) -> Self {
        Self {
            copy_text: matches!(
                kind,
                ContentKind::Text
                    | ContentKind::Credential
                    | ContentKind::Bookmark
                    | ContentKind::Note
            ),
            copy_image: matches!(kind, ContentKind::Image),
            copy_file: matches!(kind, ContentKind::File),
            copy_path: matches!(kind, ContentKind::Image | ContentKind::File),
            open_url: matches!(kind, ContentKind::Bookmark),
            reveal_sensitive: matches!(kind, ContentKind::Credential),
            edit: true,
            save: matches!(retention, RetentionState::Temporary),
            unsave: matches!(retention, RetentionState::Saved),
            delete: true,
            reorder: reorderable,
        }
    }
}
```

Also define `UnifiedField`, `UnifiedTag`, `ContentSearchHit`, `UnifiedQueryPlan`, `ContentChange`, `ContentChangedEvent`, `ContentDeleteFailedEvent`, `ContentMutation`, `DeleteUndoToken`, and `ContentRevision` using the field names in the approved spec. Implement manual `Serialize`/`Deserialize` for `UnifiedContentId` as a string so IPC never exposes source-table details.

Create `content/mod.rs` with:

```rust
pub mod catalog;
pub mod ipc;
pub mod migrations;
pub mod models;
pub mod projection;
pub mod search;
pub mod service;
```

Register `pub mod content;` in `src-tauri/src/lib.rs`.

- [ ] **Step 4: Run model tests**

Run:

```powershell
cargo test content::models::tests
```

Expected: all three model tests pass.

- [ ] **Step 5: Commit the contract**

```powershell
git add src-tauri/src/content/mod.rs src-tauri/src/content/models.rs src-tauri/src/lib.rs
git commit -m "add unified content domain contract"
```

### Task 2: Add catalog, revision, FTS schema, and idempotent backfill

**Files:**
- Create: `src-tauri/src/content/migrations.rs`
- Create: `src-tauri/src/content/catalog.rs`
- Modify: `src-tauri/src/scratchpad/storage.rs:399-416`
- Modify: `src-tauri/src/lib.rs:767-776`
- Test: `src-tauri/src/content/migrations.rs`

- [ ] **Step 1: Write migration and backfill tests**

Use an in-memory connection with foreign keys enabled. Initialize Dock and Vault schemas, insert one Home-only Dock text, one Note Dock file, and one Vault credential, then call `ensure_content_schema` twice. Assert:

```rust
#[test]
fn backfill_is_idempotent_and_maps_existing_retention() {
    let mut conn = fixture_with_legacy_rows();
    ensure_content_schema(&mut conn, 7).unwrap();
    ensure_content_schema(&mut conn, 7).unwrap();

    let rows: Vec<(String, String, Option<String>)> = conn
        .prepare(
            "SELECT unified_id, retention_state, cleanup_at
             FROM content_catalog ORDER BY unified_id",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0, "dock:home-only");
    assert_eq!(rows[0].1, "temporary");
    assert!(rows[0].2.is_some());
    assert_eq!(rows[1].0, "dock:note-file");
    assert_eq!(rows[1].1, "saved");
    assert_eq!(rows[1].2, None);
    assert_eq!(rows[2].0, "vault:credential");
    assert_eq!(rows[2].1, "saved");
}

#[test]
fn saved_order_preserves_note_order_then_appends_vault() {
    let mut conn = fixture_with_legacy_rows();
    ensure_content_schema(&mut conn, 7).unwrap();

    let ids = catalog_ids_for_scope(&conn, BrowseScope::Saved).unwrap();
    assert_eq!(ids, vec!["dock:note-file", "vault:credential"]);
}
```

- [ ] **Step 2: Verify the migration test fails**

Run:

```powershell
cargo test content::migrations::tests -- --nocapture
```

Expected: compilation fails because `ensure_content_schema` and catalog queries do not exist.

- [ ] **Step 3: Implement schema version 3 and backfill**

Use this migration SQL exactly:

```rust
const CONTENT_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS content_catalog (
    unified_id TEXT PRIMARY KEY,
    source TEXT NOT NULL CHECK (source IN ('dock', 'vault')),
    source_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (
        kind IN ('text', 'image', 'file', 'credential', 'bookmark', 'note')
    ),
    retention_state TEXT NOT NULL CHECK (
        retention_state IN ('temporary', 'saved')
    ),
    retention_changed_at TEXT NOT NULL,
    cleanup_at TEXT,
    inbox_position REAL,
    saved_position REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(source, source_id)
);

CREATE INDEX IF NOT EXISTS idx_content_catalog_retention_order
ON content_catalog(retention_state, inbox_position, saved_position);

CREATE INDEX IF NOT EXISTS idx_content_catalog_updated
ON content_catalog(updated_at DESC, unified_id ASC);

CREATE TABLE IF NOT EXISTS content_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    revision INTEGER NOT NULL
);

INSERT OR IGNORE INTO content_state(singleton, revision) VALUES (1, 0);

CREATE TABLE IF NOT EXISTS content_pending_deletes (
    token TEXT PRIMARY KEY,
    unified_id TEXT NOT NULL UNIQUE
        REFERENCES content_catalog(unified_id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'failed'))
);

CREATE INDEX IF NOT EXISTS idx_content_pending_deletes_expiry
ON content_pending_deletes(status, expires_at ASC);

CREATE VIRTUAL TABLE IF NOT EXISTS content_fts USING fts5(
    unified_id UNINDEXED,
    title,
    body,
    tags,
    aliases,
    tokenize = 'unicode61'
);
"#;
```

`ensure_content_schema(conn, cleanup_days)` must:

1. call `ensure_schema(conn, &[Migration::new(3, "unified content catalog", CONTENT_SCHEMA_SQL)])` after Dock version 1/2 and Vault schema initialization;
2. open one transaction;
3. insert missing Dock catalog rows, deriving saved state from `note_entries` membership and temporary state otherwise;
4. calculate temporary `cleanup_at` from `retention_changed_at + cleanup_days`; `cleanup_days = 0` means cleanup is due immediately on the next startup cleanup pass, matching the existing preference;
5. copy Home and Note `sort_order` into `inbox_position` and `saved_position`;
6. append Vault rows after the current maximum saved position in `updated_at DESC, id ASC` order;
7. build missing FTS projections;
8. commit and leave existing rows unchanged on the second call.

Change `ensure_dock_schema` to initialize only versions 1/2; remove its call to `cleanup_home_on_startup`. In `init_db` call, in order:

```rust
scratchpad::storage::ensure_dock_schema(&mut conn)
    .expect("Failed to init dock schema");
vault::storage::ensure_vault_schema(&mut conn)
    .expect("Failed to init vault schema");
content::migrations::ensure_content_schema(&mut conn, cleanup_days)
    .expect("Failed to init unified content schema");
content::service::cleanup_expired(&mut conn, chrono::Utc::now())
    .expect("Failed to clean expired content");
```

Update existing Rust fixtures from `ensure_dock_schema(&mut conn, 0)` to `ensure_dock_schema(&mut conn)`.

- [ ] **Step 4: Run migration and legacy storage tests**

Run:

```powershell
cargo test content::migrations::tests
cargo test scratchpad::storage::tests
cargo test vault::storage::tests
```

Expected: migration tests and both legacy storage suites pass; rerunning schema setup does not duplicate catalog rows.

- [ ] **Step 5: Commit the migration**

```powershell
git add src-tauri/src/content/migrations.rs src-tauri/src/content/catalog.rs src-tauri/src/scratchpad/storage.rs src-tauri/src/lib.rs
git commit -m "add unified content catalog migration"
```

### Task 3: Build safe projections and deterministic local search

**Files:**
- Create: `src-tauri/src/content/projection.rs`
- Create: `src-tauri/src/content/search.rs`
- Modify: `src-tauri/src/content/catalog.rs`
- Test: `src-tauri/src/content/projection.rs`
- Test: `src-tauri/src/content/search.rs`

- [ ] **Step 1: Write projection privacy and ranking tests**

Create fixtures containing:

- Dock text `"数据库维护窗口"`;
- Dock file `"上线清单.pdf"`;
- Vault credential with username `"alice"` and sensitive password `"NeverIndexMe"`;
- Vault bookmark tagged `"生产环境"`;
- AI alias `"prod console"`.

Add:

```rust
#[test]
fn projection_indexes_useful_fields_but_excludes_sensitive_values() {
    let conn = fixture_with_all_kinds();
    let credential = build_search_document(&conn, "vault:credential-1").unwrap();

    assert!(credential.body.contains("alice"));
    assert!(!credential.body.contains("NeverIndexMe"));
    assert!(!credential.title.contains("NeverIndexMe"));
}

#[test]
fn unified_search_finds_temporary_and_saved_sources() {
    let conn = fixture_with_all_kinds();
    let maintenance = search_local(&conn, "维护", None, 20).unwrap();
    let production = search_local(&conn, "生产", None, 20).unwrap();

    assert_eq!(maintenance[0].summary.id, "dock:text-1");
    assert_eq!(production[0].summary.id, "vault:bookmark-1");
}

#[test]
fn equal_scores_use_updated_at_then_stable_id() {
    let conn = fixture_with_equal_score_rows();
    let hits = search_local(&conn, "console", None, 20).unwrap();
    let ids: Vec<&str> = hits.iter().map(|hit| hit.summary.id.as_str()).collect();

    assert_eq!(ids, vec!["vault:newer", "dock:a", "dock:b"]);
}
```

- [ ] **Step 2: Run tests and verify the red state**

Run:

```powershell
cargo test content::projection::tests -- --nocapture
cargo test content::search::tests -- --nocapture
```

Expected: compilation fails because `build_search_document` and `search_local` are not implemented.

- [ ] **Step 3: Implement projection generation and search**

Define:

```rust
pub struct SearchDocument {
    pub unified_id: String,
    pub title: String,
    pub body: String,
    pub tags: String,
    pub aliases: String,
}

pub fn replace_projection(
    conn: &Connection,
    document: &SearchDocument,
) -> StorageResult<()> {
    conn.execute(
        "DELETE FROM content_fts WHERE unified_id = ?1",
        params![document.unified_id],
    )?;
    conn.execute(
        "INSERT INTO content_fts(unified_id, title, body, tags, aliases)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            document.unified_id,
            document.title,
            document.body,
            document.tags,
            document.aliases
        ],
    )?;
    Ok(())
}
```

`build_search_document` must switch on `content_catalog.source`:

- Dock text summary title: persisted title, otherwise the first non-empty line truncated to 80 Unicode characters, otherwise an empty string for localized UI fallback.
- Dock image/file summary title: persisted title, otherwise file name, otherwise an empty string for localized UI fallback.
- Dock: title, free-form text, and file name are local-searchable; file paths are excluded from the preview and indexed body.
- Vault: title, notes, tags, AI summary, aliases, and fields where `is_sensitive = 0` are searchable.
- Never copy a Vault field where `is_sensitive = 1` into any returned `String`.
- Preview generation is capped at 160 Unicode characters, strips control characters, and never uses a sensitive structured value. UI components provide localized kind-specific “未命名” text when summary title is empty.

`search_local(conn, query, plan, limit)` must:

- return browse-order summaries when the normalized query and plan terms are empty;
- query `content_fts MATCH` with escaped Unicode tokens;
- add exact title, prefix, tag, body, and alias weights in descending order;
- filter kinds and dates from `UnifiedQueryPlan`;
- return `SearchSource::Local` and add `SearchSource::AiExpanded` only when plan aliases/keywords contributed;
- sort by score descending, `updated_at DESC`, and `unified_id ASC`;
- clamp limit to `1..=100`.

Use constants so ranking is reviewable:

```rust
const EXACT_TITLE_WEIGHT: f64 = 120.0;
const TITLE_PREFIX_WEIGHT: f64 = 80.0;
const TAG_WEIGHT: f64 = 55.0;
const BODY_WEIGHT: f64 = 30.0;
const ALIAS_WEIGHT: f64 = 20.0;
```

- [ ] **Step 4: Run projection, search, and Vault compatibility tests**

Run:

```powershell
cargo test content::projection::tests
cargo test content::search::tests
cargo test vault::search::tests
```

Expected: all suites pass; the sensitive literal is absent from the generated unified document.

- [ ] **Step 5: Commit search foundation**

```powershell
git add src-tauri/src/content/projection.rs src-tauri/src/content/search.rs src-tauri/src/content/catalog.rs
git commit -m "add unified local content search"
```

### Task 4: Implement lifecycle, ordering, committed delete, and cleanup services

**Files:**
- Create: `src-tauri/src/content/service.rs`
- Modify: `src-tauri/src/content/catalog.rs`
- Modify: `src-tauri/src/scratchpad/storage.rs`
- Modify: `src-tauri/src/vault/storage.rs`
- Test: `src-tauri/src/content/service.rs`

- [ ] **Step 1: Write observable lifecycle tests**

Add tests that call only the public service:

```rust
#[test]
fn save_and_unsave_share_one_retention_model() {
    let mut conn = fixture_with_temporary_dock_and_saved_vault();

    save(&mut conn, "dock:text-1").unwrap();
    assert_eq!(retention(&conn, "dock:text-1"), "saved");
    assert!(dock_note_membership_exists(&conn, "text-1"));

    unsave(&mut conn, "vault:credential-1", 7).unwrap();
    assert_eq!(retention(&conn, "vault:credential-1"), "temporary");
    assert!(cleanup_at(&conn, "vault:credential-1").is_some());

    save(&mut conn, "vault:credential-1").unwrap();
    assert_eq!(cleanup_at(&conn, "vault:credential-1"), None);
}

#[test]
fn cleanup_deletes_only_expired_temporary_content() {
    let mut conn = fixture_with_expired_and_saved_rows();
    let report = cleanup_expired(&mut conn, fixed_now()).unwrap();

    assert_eq!(report.changes.len(), 1);
    assert_eq!(report.changes[0].id, "dock:expired");
    assert!(content_exists(&conn, "dock:saved"));
    assert!(content_exists(&conn, "vault:future"));
}

#[test]
fn delete_commits_payload_catalog_projection_and_revision_atomically() {
    let mut conn = fixture_with_saved_vault();
    let revision_before = current_revision(&conn).unwrap();
    let mutation = delete(&mut conn, "vault:credential-1").unwrap();

    assert!(!content_exists(&conn, "vault:credential-1"));
    assert!(!payload_exists(&conn, "credential-1"));
    assert!(!fts_exists(&conn, "vault:credential-1"));
    assert_eq!(mutation.revision, revision_before + 1);
    assert_eq!(mutation.changes[0].operation, ContentOperation::Deleted);
}
```

- [ ] **Step 2: Verify service tests fail**

Run:

```powershell
cargo test content::service::tests -- --nocapture
```

Expected: compilation fails because the lifecycle service functions do not exist.

- [ ] **Step 3: Implement the transactional service**

Every public mutation returns:

```rust
pub struct ContentMutation<T> {
    pub value: T,
    pub revision: i64,
    pub changes: Vec<ContentChange>,
}
```

Implement these exact entry points:

```rust
pub fn list(
    conn: &Connection,
    scope: BrowseScope,
    kind: Option<ContentKind>,
) -> StorageResult<Vec<ContentSummary>>;

pub fn detail(
    conn: &Connection,
    id: &str,
) -> StorageResult<ContentDetail>;

pub fn search(
    conn: &Connection,
    query: &str,
    plan: Option<&UnifiedQueryPlan>,
    limit: usize,
) -> StorageResult<Vec<ContentSearchHit>>;

pub fn save(
    conn: &mut Connection,
    id: &str,
) -> StorageResult<ContentMutation<ContentSummary>>;

pub fn unsave(
    conn: &mut Connection,
    id: &str,
    cleanup_days: i64,
) -> StorageResult<ContentMutation<ContentSummary>>;

pub fn reorder(
    conn: &mut Connection,
    scope: BrowseScope,
    ordered_ids: &[String],
) -> StorageResult<ContentMutation<()>>;

pub fn delete(
    conn: &mut Connection,
    id: &str,
) -> StorageResult<ContentMutation<()>>;

pub fn cleanup_expired(
    conn: &mut Connection,
    now: DateTime<Utc>,
) -> StorageResult<ContentMutation<usize>>;
```

Rules enforced inside these functions:

- `list(Temporary)` sorts `inbox_position ASC`, `list(Saved)` sorts `saved_position ASC`, and `list(All)` sorts `updated_at DESC, unified_id ASC`.
- New lightweight temporary content receives `min(inbox_position) - 1`, so it appears at the top without renumbering the whole scope.
- New organized capture and every save transition receive `min(saved_position) - 1`; unsave receives `min(inbox_position) - 1`.
- Dock `save` inserts Note membership and clears `cleanup_at`; Dock `unsave` removes Note membership, ensures Home membership, and sets cleanup timing from the transition time.
- Vault `save`/`unsave` only changes catalog retention; its structured payload remains in `vault_*`.
- `reorder` accepts only Temporary or Saved, rejects All, validates the supplied ID set exactly matches the current scope, and writes sequential positions in one transaction.
- `delete` atomically removes the payload, memberships/fields, catalog row, FTS row, and increments revision. The pending undo window is owned by the IPC layer in Task 7, so `delete` is called only after that window expires.
- `restore` rejects an occupied unified ID and restores the exact source ID, membership/retention, positions, fields/tags/AI metadata, and FTS projection.
- `cleanup_expired` selects `retention_state = 'temporary' AND cleanup_at <= now`. Delete attached files only after the database commit; a file deletion failure is logged without resurrecting the row.
- Call `catalog::bump_revision(&tx)` exactly once per public mutation transaction.

- [ ] **Step 4: Run service and full Rust tests**

Run:

```powershell
cargo test content::service::tests
cargo test
```

Expected: lifecycle tests pass and the existing Rust suite has no regression.

- [ ] **Step 5: Commit application services**

```powershell
git add src-tauri/src/content/service.rs src-tauri/src/content/catalog.rs src-tauri/src/scratchpad/storage.rs src-tauri/src/vault/storage.rs
git commit -m "add unified content lifecycle service"
```

### Task 5: Keep every legacy Dock write path synchronized

**Files:**
- Modify: `src-tauri/src/scratchpad/storage.rs:265-548`
- Modify: `src-tauri/src/scratchpad/assets.rs`
- Modify: `src-tauri/src/lib.rs:257-336`
- Modify: `src-tauri/src/lib.rs:521-558`
- Test: `src-tauri/src/scratchpad/storage.rs`
- Test: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add compatibility-write regression tests**

For `create_text_entry`, import image/file, `update_entry_text`, `rename_entry`, `add_to_note`, `remove_from_view`, and `reorder_entries`, assert after each successful call:

```rust
fn assert_dock_projection_matches(conn: &Connection, source_id: &str) {
    let unified_id = format!("dock:{source_id}");
    let catalog_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM content_catalog WHERE unified_id = ?1",
            params![unified_id],
            |row| row.get(0),
        )
        .unwrap();
    let fts_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM content_fts WHERE unified_id = ?1",
            params![unified_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(catalog_count, 1);
    assert_eq!(fts_count, 1);
}

#[test]
fn legacy_dock_writes_cannot_bypass_unified_projection() {
    let mut conn = initialized_fixture();
    let created = create_text_entry(&mut conn, EntryView::Home, "alpha", "manual").unwrap();
    assert_dock_projection_matches(&conn, &created.id);

    update_entry_text(&mut conn, &created.id, "beta").unwrap();
    assert_eq!(fts_body(&conn, &format!("dock:{}", created.id)), "beta");

    add_to_note(&mut conn, &created.id).unwrap();
    assert_eq!(retention(&conn, &format!("dock:{}", created.id)), "saved");
}
```

- [ ] **Step 2: Run tests and observe the stale catalog failure**

Run:

```powershell
cargo test legacy_dock_writes_cannot_bypass_unified_projection -- --nocapture
```

Expected: the test fails because a legacy update changes `entries` without refreshing `content_fts` or retention.

- [ ] **Step 3: Add transaction-local hooks**

Add these internal helpers to `scratchpad/storage.rs` and call them before every write transaction commits:

```rust
fn sync_dock_content(
    conn: &Connection,
    source_id: &str,
    retention_changed: bool,
) -> StorageResult<i64> {
    crate::content::catalog::upsert_dock(conn, source_id, retention_changed)?;
    let id = UnifiedContentId::new(ContentSource::Dock, source_id);
    let document = crate::content::projection::build_search_document(conn, id.as_str())?;
    crate::content::projection::replace_projection(conn, &document)?;
    crate::content::catalog::bump_revision(conn)
}

fn delete_dock_content(conn: &Connection, source_id: &str) -> StorageResult<i64> {
    let id = UnifiedContentId::new(ContentSource::Dock, source_id);
    crate::content::projection::delete_projection(conn, id.as_str())?;
    crate::content::catalog::delete_row(conn, id.as_str())?;
    crate::content::catalog::bump_revision(conn)
}
```

Do not start a nested transaction in these helpers. Modify the existing write functions to return or expose the committed revision to their IPC wrapper. Preserve the existing `DockEntry` response shape.

For `add_to_note` and `remove_from_view`, route favorite semantics through `content::service::save`/`unsave` so `cleanup_at` uses the retention transition time. Removing from Home must not delete an item that remains saved. Removing the final temporary membership uses unified delete.

After each successful command in `lib.rs`, emit a `content-changed` event with the actual namespaced ID and operation. File/image import uses the same `Created` operation after the asset/storage call succeeds.

- [ ] **Step 4: Run Dock, IPC, and complete Rust tests**

Run:

```powershell
cargo test scratchpad::storage::tests
cargo test tests::ipc_
cargo test
```

Expected: all legacy Dock writes keep one catalog row and one safe FTS row, and all Rust tests pass.

- [ ] **Step 5: Commit Dock compatibility**

```powershell
git add src-tauri/src/scratchpad/storage.rs src-tauri/src/scratchpad/assets.rs src-tauri/src/lib.rs
git commit -m "synchronize dock writes with content catalog"
```

### Task 6: Keep every Vault and AI write path synchronized

**Files:**
- Modify: `src-tauri/src/vault/storage.rs:78-860`
- Modify: `src-tauri/src/vault/ipc/entries.rs`
- Modify: `src-tauri/src/vault/ipc/capture.rs`
- Modify: `src-tauri/src/vault/jobs.rs:79-330`
- Test: `src-tauri/src/vault/storage.rs`
- Test: `src-tauri/src/vault/jobs.rs`

- [ ] **Step 1: Add structured-write and privacy regression tests**

```rust
#[test]
fn vault_writes_refresh_unified_projection_without_sensitive_fields() {
    let mut conn = initialized_fixture();
    let detail = create_entry(
        &mut conn,
        &credential_input("Server", "alice", "NeverIndexMe"),
    )
    .unwrap();
    let id = format!("vault:{}", detail.entry.id);

    assert_eq!(retention(&conn, &id), "saved");
    assert!(fts_body(&conn, &id).contains("alice"));
    assert!(!fts_body(&conn, &id).contains("NeverIndexMe"));

    set_manual_tags(&mut conn, &detail.entry.id, &["production".into()]).unwrap();
    assert!(fts_tags(&conn, &id).contains("production"));
}

#[test]
fn ai_metadata_write_refreshes_aliases_without_exposing_sensitive_values() {
    let mut conn = initialized_credential_fixture();
    set_ai_metadata(&mut conn, &ready_metadata("credential-1", "prod console")).unwrap();

    assert!(fts_aliases(&conn, "vault:credential-1").contains("prod console"));
    assert!(!fts_document(&conn, "vault:credential-1").contains("NeverIndexMe"));
}
```

- [ ] **Step 2: Run tests and verify the projection is stale**

Run:

```powershell
cargo test vault_writes_refresh_unified_projection_without_sensitive_fields -- --nocapture
cargo test ai_metadata_write_refreshes_aliases_without_exposing_sensitive_values -- --nocapture
```

Expected: at least one assertion fails because current Vault mutations update only `vault_fts`.

- [ ] **Step 3: Add Vault transaction-local hooks and background emission**

Add:

```rust
fn sync_vault_content(conn: &Connection, source_id: &str) -> StorageResult<i64> {
    crate::content::catalog::upsert_vault(conn, source_id)?;
    let id = UnifiedContentId::new(ContentSource::Vault, source_id);
    let document = crate::content::projection::build_search_document(conn, id.as_str())?;
    crate::content::projection::replace_projection(conn, &document)?;
    crate::content::catalog::bump_revision(conn)
}
```

Call it in the same transaction as:

- `create_entry`, `update_entry`, `create_from_capture`;
- manual/AI tag replacement and removal;
- AI metadata ready/pending/error changes;
- delete, using a delete hook before commit.

Organized capture is always inserted into `content_catalog` with `retention_state = 'saved'` and `cleanup_at = NULL`, regardless of AI configuration or enrichment outcome.

After successful IPC writes, emit `Created`, `Updated`, or `Deleted`. In `jobs.rs`, emit `Updated` after committing AI metadata or error state. The event contains only unified ID, operation, and revision.

- [ ] **Step 4: Run Vault privacy, job, capture, and full Rust tests**

Run:

```powershell
cargo test vault::storage::tests
cargo test vault::jobs::tests
cargo test vault::ipc::capture::tests
cargo test
```

Expected: all tests pass; grep of test output and fixtures shows no emitted event containing the sensitive literal.

- [ ] **Step 5: Commit Vault compatibility**

```powershell
git add src-tauri/src/vault/storage.rs src-tauri/src/vault/ipc/entries.rs src-tauri/src/vault/ipc/capture.rs src-tauri/src/vault/jobs.rs
git commit -m "synchronize vault writes with content catalog"
```

### Task 7: Expose unified IPC and recoverable content events

**Files:**
- Create: `src-tauri/src/content/ipc.rs`
- Modify: `src-tauri/src/lib.rs:799-850`
- Test: `src-tauri/src/content/ipc.rs`
- Test: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write IPC behavior tests**

Separate service logic from Tauri macros with plain handler helpers, then test:

```rust
#[test]
fn event_payload_contains_monotonic_revision_and_namespaced_changes() {
    let event = content_changed_event(
        14,
        vec![ContentChange {
            id: "dock:de-1".to_string(),
            operation: ContentOperation::Retention,
        }],
    );

    assert_eq!(event.revision, 14);
    assert_eq!(event.changes[0].id, "dock:de-1");
}

#[test]
fn revision_reports_missed_backend_changes() {
    let mut conn = initialized_fixture();
    assert_eq!(current_revision(&conn).unwrap(), 0);
    create_temporary_text(&mut conn, "alpha").unwrap();
    assert_eq!(current_revision(&conn).unwrap(), 1);
}

#[test]
fn pending_delete_can_be_cancelled_before_expiry_only_once() {
    let mut conn = initialized_fixture();
    let now = Utc::now();
    let token = prepare_delete(
        &mut conn,
        "vault:credential-1",
        now,
        Duration::seconds(10),
    )
    .unwrap();

    assert_eq!(
        cancel_pending_delete(
            &mut conn,
            &token.token,
            now + Duration::seconds(9),
        )
        .unwrap()
        .id,
        "vault:credential-1",
    );
    assert!(
        cancel_pending_delete(
            &mut conn,
            &token.token,
            now + Duration::seconds(9),
        )
        .is_err()
    );
}

#[test]
fn expired_pending_delete_commits_after_restart() {
    let (path, mut conn) = initialized_file_fixture();
    let now = Utc::now();
    let token = prepare_delete(
        &mut conn,
        "dock:de-1",
        now,
        Duration::seconds(10),
    )
    .unwrap();

    assert!(commit_expired_deletes(&mut conn, now + Duration::seconds(9))
        .unwrap()
        .is_empty());
    drop(conn);

    let mut reopened = open_fixture(&path);
    let mutations =
        commit_expired_deletes(&mut reopened, now + Duration::seconds(10)).unwrap();
    assert_eq!(mutations[0].token, token.token);
    assert!(!content_exists(&reopened, "dock:de-1"));
}
```

- [ ] **Step 2: Verify IPC tests fail**

Run:

```powershell
cargo test content::ipc::tests -- --nocapture
```

Expected: compilation fails because the event and revision handlers are missing.

- [ ] **Step 3: Implement and register exact commands**

Expose:

```rust
#[tauri::command]
pub fn ipc_content_revision(state: State<AppState>) -> Result<ContentRevision, String>;

#[tauri::command]
pub fn ipc_content_list(
    state: State<AppState>,
    scope: BrowseScope,
    kind: Option<ContentKind>,
) -> Result<Vec<ContentSummary>, String>;

#[tauri::command]
pub fn ipc_content_detail(
    state: State<AppState>,
    id: String,
) -> Result<ContentDetail, String>;

#[tauri::command]
pub fn ipc_content_search_local(
    state: State<AppState>,
    query: String,
    plan: Option<UnifiedQueryPlan>,
    limit: Option<usize>,
) -> Result<Vec<ContentSearchHit>, String>;

#[tauri::command]
pub fn ipc_content_save(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<ContentSummary, String>;

#[tauri::command]
pub fn ipc_content_unsave(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<ContentSummary, String>;

#[tauri::command]
pub fn ipc_content_reorder(
    app: AppHandle,
    state: State<AppState>,
    scope: BrowseScope,
    ordered_ids: Vec<String>,
) -> Result<(), String>;

#[tauri::command]
pub fn ipc_content_delete(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<DeleteUndoToken, String>;

#[tauri::command]
pub fn ipc_content_restore(
    app: AppHandle,
    state: State<AppState>,
    token: String,
) -> Result<ContentSummary, String>;
```

`ipc_content_unsave` loads `auto_cleanup_days` from Dock preferences inside the database lock.

`ipc_content_delete` implements the approved deferred-delete behavior:

1. validate the opaque ID and load its current summary without changing SQLite;
2. insert `token + unified_id + created_at + expires_at + status='pending'` into `content_pending_deletes` in a short transaction without changing payload/catalog/revision;
3. return `DeleteUndoToken { token, expires_at }` so the initiating UI can hide the item optimistically;
4. start one Tauri async task that wakes at expiry, opens the database lock, verifies the persisted token is due, and deletes payload/catalog/FTS plus the pending row in one transaction;
5. emit `content-changed` with `Deleted` only after the real transaction commits;
6. if the commit fails, log the technical error locally and emit `content-delete-failed { token, id, code: "content_delete_commit_failed" }` so the initiating UI restores its optimistic row and uses localized copy.

`ipc_content_restore` is the visible Undo operation. Before expiry it atomically removes the pending row and returns the still-existing summary; it does not rewrite payload data or increment revision because deletion never committed. After expiry or after one successful cancellation it returns the localized error code `content_delete_undo_expired`.

On application setup, call `resume_pending_deletes(app.handle())` after `AppState` is managed. It commits already-expired `status='pending'` rows and schedules future pending rows, so exiting during the 10-second interval does not silently cancel the user's deletion. A commit failure rolls back content deletion, changes that token to `status='failed'` in a separate short transaction, logs the technical cause, and emits the safe failure code. Startup deletes old failed rows without touching content; failed tokens are never retried.

The pending table stores only token, namespaced ID, and timestamps. No password, note body, file path, structured field, or payload snapshot is duplicated into WebView or pending state. Each actual mutation emits only after the transaction returns success:

```rust
pub fn emit_content_changed(
    app: &AppHandle,
    revision: i64,
    changes: Vec<ContentChange>,
) {
    if let Err(error) = app.emit(
        "content-changed",
        ContentChangedEvent { revision, changes },
    ) {
        eprintln!("content-changed emit failed at revision {revision}: {error}");
    }
}
```

An emit failure is logged after the committed mutation and does not turn a successful save/delete into a false UI failure. The next focus/revision comparison repairs the UI. Register `content-delete-failed` as a typed frontend event in Task 8.

Register every command in `tauri::generate_handler!`.

- [ ] **Step 4: Run IPC and complete Rust tests**

Run:

```powershell
cargo test content::ipc::tests
cargo test
```

Expected: commands compile, revisions increase monotonically, and the complete Rust suite passes.

- [ ] **Step 5: Commit unified IPC**

```powershell
git add src-tauri/src/content/ipc.rs src-tauri/src/lib.rs
git commit -m "expose unified content ipc and events"
```

### Task 8: Add the TypeScript contract and API wrapper

**Files:**
- Create: `src/lib/types/content.ts`
- Create: `src/lib/api/content.ts`
- Create: `src/lib/api/content.test.ts`

- [ ] **Step 1: Write failing API contract tests**

Mock Tauri `invoke` and `listen` following `src/lib/api/dock.test.ts`, then add:

```typescript
it('lists and searches every content kind through unified commands', async () => {
  await contentApi.list('all', null)
  expect(invoke).toHaveBeenCalledWith('ipc_content_list', {
    scope: 'all',
    kind: null,
  })

  await contentApi.searchLocal('生产', null, 40)
  expect(invoke).toHaveBeenCalledWith('ipc_content_search_local', {
    query: '生产',
    plan: null,
    limit: 40,
  })
})

it('subscribes to the recoverable content event', async () => {
  const callback = vi.fn()
  await onContentChanged(callback)
  expect(listen).toHaveBeenCalledWith('content-changed', expect.any(Function))
})

it('subscribes to deferred delete commit failures', async () => {
  const callback = vi.fn()
  await onContentDeleteFailed(callback)
  expect(listen).toHaveBeenCalledWith('content-delete-failed', expect.any(Function))
})
```

- [ ] **Step 2: Verify frontend tests fail**

Run:

```powershell
pnpm exec vitest run src/lib/api/content.test.ts
```

Expected: test collection fails because `$lib/api/content` and `$lib/types/content` do not exist.

- [ ] **Step 3: Implement mirrored types and typed calls**

Define the exact frontend union types:

```typescript
export type ContentKind =
  | 'text'
  | 'image'
  | 'file'
  | 'credential'
  | 'bookmark'
  | 'note'

export type RetentionState = 'temporary' | 'saved'
export type BrowseScope = 'temporary' | 'all' | 'saved'
export type ContentOperation =
  | 'created'
  | 'updated'
  | 'retention'
  | 'reordered'
  | 'deleted'
  | 'restored'

export interface ContentSummary {
  id: string
  kind: ContentKind
  retention: RetentionState
  title: string
  preview: string | null
  createdAt: string
  updatedAt: string
  cleanupAt: string | null
  capabilities: ContentCapabilities
}

export interface ContentChangedEvent {
  revision: number
  changes: Array<{ id: string; operation: ContentOperation }>
}
```

Mirror every Rust IPC type, including tagged detail variants, query plan, search hit/source, revision, `DeleteUndoToken`, and `ContentDeleteFailedEvent`. The undo interface exposes only `token` and `expiresAt` to the UI; `content_pending_deletes` contains only token, namespaced ID, and timestamps.

Implement `contentApi` methods matching all nine commands in Task 7 and:

```typescript
export function onContentChanged(
  callback: (event: ContentChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<ContentChangedEvent>('content-changed', (event) => {
    callback(event.payload)
  })
}
```

Add a matching `onContentDeleteFailed` wrapper that listens to `content-delete-failed` and forwards `{ token, id, code }`. Keep both event wrappers in `content.ts` so Main and Quick never duplicate event names.

- [ ] **Step 4: Run API tests and type checking**

Run:

```powershell
pnpm exec vitest run src/lib/api/content.test.ts
pnpm check
```

Expected: API tests pass; `svelte-check` reports no new errors.

- [ ] **Step 5: Commit frontend contract**

```powershell
git add src/lib/types/content.ts src/lib/api/content.ts src/lib/api/content.test.ts src-tauri/src/content/ipc.rs src-tauri/src/content/service.rs src-tauri/src/lib.rs
git commit -m "add typed unified content api"
```

### Task 9: Validate upgrades, compatibility, privacy, and failure recovery

**Files:**
- Modify: `src-tauri/src/content/migrations.rs`
- Modify: `src-tauri/src/content/service.rs`
- Modify: `src-tauri/src/content/ipc.rs`
- Create: `src-tauri/examples/create_validation_fixtures.rs`
- Create: `docs/superpowers/reports/2026-07-18-unified-content-foundation-verification.md`

- [ ] **Step 1: Add upgrade and rollback-boundary fixtures**

Add tests for:

```rust
#[test]
fn upgrade_preserves_all_legacy_payload_counts_and_membership() {
    let mut conn = fixture_from_pre_content_schema();
    let before = legacy_counts(&conn);
    ensure_content_schema(&mut conn, 30).unwrap();
    let after = legacy_counts(&conn);

    assert_eq!(after, before);
    assert_eq!(catalog_count(&conn), before.unique_content_count);
}

#[test]
fn failed_projection_write_rolls_back_payload_and_revision() {
    let mut conn = initialized_fixture();
    break_content_fts_for_test(&conn);
    let revision_before = current_revision(&conn).unwrap();

    assert!(create_temporary_text(&mut conn, "must rollback").is_err());
    assert!(!dock_text_exists(&conn, "must rollback"));
    assert_eq!(current_revision(&conn).unwrap(), revision_before);
}
```

Create `create_validation_fixtures.rs` as a deterministic developer tool:

```rust
use std::error::Error;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: create_validation_fixtures <output-directory>")?;
    std::fs::create_dir_all(&output)?;
    create_legacy_fixture(&output.join("legacy-validation.sqlite3"))?;
    create_fresh_fixture(&output.join("fresh-validation.sqlite3"))?;
    Ok(())
}

fn create_legacy_fixture(path: &Path) -> Result<(), Box<dyn Error>> {
    ensure_new_path(path)?;
    let mut conn = rusqlite::Connection::open(path)?;
    soma_scratchpad::scratchpad::storage::ensure_dock_schema(&mut conn)?;
    soma_scratchpad::vault::storage::ensure_vault_schema(&mut conn)?;
    insert_deterministic_legacy_rows(&mut conn)?;
    Ok(())
}

fn create_fresh_fixture(path: &Path) -> Result<(), Box<dyn Error>> {
    ensure_new_path(path)?;
    let mut conn = rusqlite::Connection::open(path)?;
    soma_scratchpad::scratchpad::storage::ensure_dock_schema(&mut conn)?;
    soma_scratchpad::vault::storage::ensure_vault_schema(&mut conn)?;
    soma_scratchpad::content::migrations::ensure_content_schema(&mut conn, 30)?;
    insert_deterministic_current_rows(&mut conn)?;
    Ok(())
}

fn ensure_new_path(path: &Path) -> Result<(), Box<dyn Error>> {
    if path.exists() {
        return Err(format!("refusing to overwrite {}", path.display()).into());
    }
    Ok(())
}
```

Implement both insert functions with fixed IDs/timestamps and six kinds. Legacy rows must include Home-only, Note-only, dual-membership, manual order, sensitive credential fields, tags, and AI metadata. Current rows must include both retention states and one pending-delete row. The tool refuses to overwrite either named database, so validation always starts by creating a new output directory.

- [ ] **Step 2: Run the two tests before hardening**

Run:

```powershell
cargo test upgrade_preserves_all_legacy_payload_counts_and_membership -- --nocapture
cargo test failed_projection_write_rolls_back_payload_and_revision -- --nocapture
```

Expected: the upgrade test passes; the forced FTS failure test fails if any write commits payload before projection/revision.

- [ ] **Step 3: Close any atomicity gap and write the verification report**

Move catalog/projection calls inside the source transaction wherever the forced-failure test detects a partial write. Record:

- test database starting schema/version and row counts;
- post-upgrade row counts and retention mapping;
- exact privacy test literals and proof they are absent from FTS/events;
- command output summaries;
- explicit statement that UI behavior is intentionally unchanged in this plan.

Generate the reusable non-production databases:

```powershell
Push-Location src-tauri
cargo run --example create_validation_fixtures -- ..\test-data\unified-validation
Pop-Location
```

Expected: two new fixture databases are created without reading the user's application data.

Use this report skeleton with actual results:

```markdown
# Unified Content Foundation Verification

## Upgrade fixture
- Starting main schema:
- Starting Vault schema:
- Payload rows before/after:
- Catalog rows after:

## Atomicity
- Forced failure:
- Payload rollback:
- Revision rollback:

## Sensitive-data boundary
- Sensitive fixture:
- Unified FTS check:
- Event payload check:

## Commands
- Rust:
- Frontend:
- Type/build:
```

- [ ] **Step 4: Run the complete quality gate**

Run:

```powershell
pnpm test:unit
pnpm check
pnpm build
Push-Location src-tauri
cargo fmt -- --check
cargo test
Pop-Location
git diff --check
```

Expected:

- all frontend unit tests pass;
- `svelte-check` has zero errors (existing warnings may remain unchanged);
- production build passes;
- Rust formatting check and all Rust tests pass;
- `git diff --check` reports no whitespace errors.

- [ ] **Step 5: Commit foundation verification**

```powershell
git add src-tauri/src/content/migrations.rs src-tauri/src/content/service.rs src-tauri/src/content/ipc.rs src-tauri/examples/create_validation_fixtures.rs docs/superpowers/reports/2026-07-18-unified-content-foundation-verification.md
git commit -m "verify unified content foundation"
```

## Plan acceptance gate

Do not start the main-workspace plan until all statements are true:

- Existing screens behave as before.
- Every existing Dock/Vault/capture/AI mutation synchronizes catalog, safe FTS, and revision transactionally.
- Old Home-only items are temporary; old Note/favorite and every Vault item are saved.
- Temporary Dock and Vault items are searchable.
- Structured sensitive fields are absent from unified FTS, events, logs, and frontend undo payloads.
- Save/unsave and cleanup work for both sources.
- Missed events are detectable through `ipc_content_revision`.
- Full Rust/frontend/check/build gates pass.
