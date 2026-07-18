use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::content::catalog::{bump_revision, top_position};
use crate::content::models::{
    ContentChange, ContentMutation, ContentOperation, ContentSource, RetentionState,
    UnifiedContentId,
};
use crate::content::projection::{build_search_document, replace_projection};
use crate::storage::error::{StorageError, StorageResult};
use crate::vault::models::{
    is_default_sensitive_key, AiMetadataStatus, BackfillStatus, CaptureDraft, EntryKind, TagSource,
    VaultAiMetadata, VaultEntry, VaultEntryDetail, VaultEntryInput, VaultEntrySummary, VaultField,
    VaultTag,
};

const VAULT_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS vault_entries (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('credential','bookmark','note')),
    title TEXT NOT NULL,
    notes TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_vault_entries_kind ON vault_entries(kind);
CREATE INDEX IF NOT EXISTS idx_vault_entries_updated ON vault_entries(updated_at DESC);

CREATE TABLE IF NOT EXISTS vault_fields (
    id TEXT PRIMARY KEY,
    entry_id TEXT NOT NULL REFERENCES vault_entries(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    is_sensitive INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    UNIQUE(entry_id, key)
);
CREATE INDEX IF NOT EXISTS idx_vault_fields_entry ON vault_fields(entry_id);

CREATE TABLE IF NOT EXISTS vault_tags (
    entry_id TEXT NOT NULL REFERENCES vault_entries(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (entry_id, tag)
);
CREATE INDEX IF NOT EXISTS idx_vault_tags_tag ON vault_tags(tag);

CREATE VIRTUAL TABLE IF NOT EXISTS vault_fts USING fts5(
    entry_id UNINDEXED,
    title,
    notes,
    searchable,
    tokenize = 'unicode61'
);
"#;

pub fn ensure_vault_schema(conn: &mut Connection) -> StorageResult<()> {
    conn.execute_batch(VAULT_SCHEMA_SQL)?;
    // v1 表已就绪后，运行版本化迁移（idempotent；已迁移则 no-op）
    crate::vault::migrations::migrate_vault_schema(conn)?;
    Ok(())
}

static ENTRY_SEQ: AtomicU64 = AtomicU64::new(0);

fn next_entry_id() -> String {
    let n = ENTRY_SEQ.fetch_add(1, Ordering::SeqCst);
    let ts = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    format!("vault-{ts}-{n}")
}

fn next_field_id() -> String {
    let n = ENTRY_SEQ.fetch_add(1, Ordering::SeqCst);
    let ts = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    format!("vf-{ts}-{n}")
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnifiedSchemaState {
    Absent,
    Complete,
}

fn unified_schema_state(conn: &Connection) -> StorageResult<UnifiedSchemaState> {
    const REQUIRED_TABLES: [&str; 4] = [
        "content_state",
        "content_catalog",
        "content_fts",
        "content_pending_deletes",
    ];
    let mut present = Vec::new();
    for table in REQUIRED_TABLES {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1
             )",
            params![table],
            |row| row.get(0),
        )?;
        if exists {
            present.push(table);
        }
    }
    match present.len() {
        0 => Ok(UnifiedSchemaState::Absent),
        count if count == REQUIRED_TABLES.len() => Ok(UnifiedSchemaState::Complete),
        _ => Err(StorageError::Migration(
            "partial unified content schema; initialization must be retried".to_string(),
        )),
    }
}

fn require_unified_schema(conn: &Connection) -> StorageResult<()> {
    match unified_schema_state(conn)? {
        UnifiedSchemaState::Complete => Ok(()),
        UnifiedSchemaState::Absent => Err(StorageError::Migration(
            "unified content schema is not initialized".to_string(),
        )),
    }
}

fn vault_unified_id(entry_id: &str) -> StorageResult<String> {
    Ok(UnifiedContentId::new(ContentSource::Vault, entry_id)
        .map_err(StorageError::Validation)?
        .as_str()
        .to_string())
}

fn sync_vault_content(conn: &Connection, source_id: &str) -> StorageResult<String> {
    let (kind, created_at, updated_at) = conn.query_row(
        "SELECT kind, created_at, updated_at FROM vault_entries WHERE id=?1",
        params![source_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    let unified_id = vault_unified_id(source_id)?;
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM content_catalog WHERE unified_id=?1)",
        params![unified_id],
        |row| row.get(0),
    )?;
    let saved_position = if exists {
        None
    } else {
        Some(top_position(conn, RetentionState::Saved)?)
    };
    conn.execute(
        "INSERT INTO content_catalog(
             unified_id, source, source_id, kind, retention_state,
             retention_changed_at, cleanup_at, inbox_position, saved_position,
             created_at, updated_at
         ) VALUES (?1, 'vault', ?2, ?3, 'saved', ?4, NULL, NULL, ?5, ?4, ?6)
         ON CONFLICT(unified_id) DO UPDATE SET
             kind=excluded.kind,
             created_at=excluded.created_at,
             updated_at=excluded.updated_at",
        params![
            unified_id,
            source_id,
            kind,
            created_at,
            saved_position,
            updated_at,
        ],
    )?;
    let document = build_search_document(conn, &unified_id)?;
    replace_projection(conn, &document)?;
    Ok(unified_id)
}

fn content_mutation<T>(
    value: T,
    revision: i64,
    unified_id: String,
    operation: ContentOperation,
) -> ContentMutation<T> {
    ContentMutation {
        value,
        revision,
        changes: vec![ContentChange {
            id: unified_id,
            operation,
        }],
    }
}

fn unchanged_content_mutation<T>(conn: &Connection, value: T) -> StorageResult<ContentMutation<T>> {
    Ok(ContentMutation {
        value,
        revision: crate::content::catalog::current_revision(conn)?,
        changes: Vec::new(),
    })
}

pub fn create_entry(
    conn: &mut Connection,
    input: &VaultEntryInput,
) -> StorageResult<VaultEntryDetail> {
    if unified_schema_state(conn)? == UnifiedSchemaState::Complete {
        return Ok(create_entry_with_revision(conn, input)?.value);
    }

    let tx = conn.transaction()?;
    let id = insert_entry(&tx, input)?;
    tx.commit()?;
    get_entry_detail(conn, &id)
}

pub fn create_entry_with_revision(
    conn: &mut Connection,
    input: &VaultEntryInput,
) -> StorageResult<ContentMutation<VaultEntryDetail>> {
    require_unified_schema(conn)?;
    let tx = conn.transaction()?;
    let id = insert_entry(&tx, input)?;
    let unified_id = sync_vault_content(&tx, &id)?;
    let revision = bump_revision(&tx)?;
    let detail = get_entry_detail(&tx, &id)?;
    tx.commit()?;
    Ok(content_mutation(
        detail,
        revision,
        unified_id,
        ContentOperation::Created,
    ))
}

fn insert_entry(conn: &Connection, input: &VaultEntryInput) -> StorageResult<String> {
    let id = next_entry_id();
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO vault_entries(id, kind, title, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, input.kind.as_str(), input.title, input.notes, now, now],
    )?;
    for (index, field) in input.fields.iter().enumerate() {
        let sensitive = field.is_sensitive || is_default_sensitive_key(&field.key);
        conn.execute(
            "INSERT INTO vault_fields(id, entry_id, key, value, is_sensitive, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                next_field_id(),
                id,
                field.key,
                field.value,
                sensitive as i32,
                index as i64,
            ],
        )?;
    }
    write_manual_tags(conn, &id, &input.manual_tags)?;
    upsert_ai_metadata_pending(conn, &id, &ai_content_hash(input))?;
    fts5_upsert(conn, &id)?;
    Ok(id)
}

pub fn list_fields(conn: &Connection, entry_id: &str) -> StorageResult<Vec<VaultField>> {
    let mut stmt = conn.prepare(
        "SELECT id, entry_id, key, value, is_sensitive, sort_order
         FROM vault_fields WHERE entry_id = ?1 ORDER BY sort_order",
    )?;
    let rows = stmt.query_map(params![entry_id], row_to_field)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(StorageError::from)
}

fn row_to_field(row: &Row) -> rusqlite::Result<VaultField> {
    Ok(VaultField {
        id: row.get("id")?,
        entry_id: row.get("entry_id")?,
        key: row.get("key")?,
        value: row.get("value")?,
        is_sensitive: row.get::<_, i32>("is_sensitive")? != 0,
        sort_order: row.get("sort_order")?,
    })
}

pub fn get_entry_by_id(conn: &Connection, id: &str) -> StorageResult<Option<VaultEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, title, notes, created_at, updated_at FROM vault_entries WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(r) = rows.next()? {
        Ok(Some(row_to_entry(r)?))
    } else {
        Ok(None)
    }
}

fn row_to_entry(row: &Row) -> rusqlite::Result<VaultEntry> {
    let kind_str: String = row.get("kind")?;
    let kind = EntryKind::parse(&kind_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bad kind",
            )),
        )
    })?;
    Ok(VaultEntry {
        id: row.get("id")?,
        kind,
        title: row.get("title")?,
        notes: row.get("notes")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn update_entry(
    conn: &mut Connection,
    id: &str,
    input: &VaultEntryInput,
) -> StorageResult<VaultEntryDetail> {
    if unified_schema_state(conn)? == UnifiedSchemaState::Complete {
        return Ok(update_entry_with_revision(conn, id, input)?.value);
    }

    let tx = conn.transaction()?;
    update_entry_rows(&tx, id, input)?;
    tx.commit()?;
    get_entry_detail(conn, id)
}

pub fn update_entry_with_revision(
    conn: &mut Connection,
    id: &str,
    input: &VaultEntryInput,
) -> StorageResult<ContentMutation<VaultEntryDetail>> {
    require_unified_schema(conn)?;
    let tx = conn.transaction()?;
    update_entry_rows(&tx, id, input)?;
    let unified_id = sync_vault_content(&tx, id)?;
    let revision = bump_revision(&tx)?;
    let detail = get_entry_detail(&tx, id)?;
    tx.commit()?;
    Ok(content_mutation(
        detail,
        revision,
        unified_id,
        ContentOperation::Updated,
    ))
}

fn update_entry_rows(conn: &Connection, id: &str, input: &VaultEntryInput) -> StorageResult<()> {
    let old_hash: Option<String> = conn
        .query_row(
            "SELECT content_hash FROM vault_ai_metadata WHERE entry_id=?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?;
    let affected = conn.execute(
        "UPDATE vault_entries SET kind=?1, title=?2, notes=?3, updated_at=?4 WHERE id=?5",
        params![
            input.kind.as_str(),
            input.title,
            input.notes,
            now_rfc3339(),
            id
        ],
    )?;
    if affected == 0 {
        return Err(StorageError::Other(format!("entry not found: {id}")));
    }
    conn.execute("DELETE FROM vault_fields WHERE entry_id=?1", params![id])?;
    for (index, field) in input.fields.iter().enumerate() {
        let sensitive = field.is_sensitive || is_default_sensitive_key(&field.key);
        conn.execute(
            "INSERT INTO vault_fields(id, entry_id, key, value, is_sensitive, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                next_field_id(),
                id,
                field.key,
                field.value,
                sensitive as i32,
                index as i64,
            ],
        )?;
    }
    write_manual_tags(conn, id, &input.manual_tags)?;
    let new_hash = ai_content_hash(input);
    if old_hash.as_deref() != Some(new_hash.as_str()) {
        conn.execute(
            "DELETE FROM vault_tags WHERE entry_id=?1 AND source='ai'",
            params![id],
        )?;
        upsert_ai_metadata_pending(conn, id, &new_hash)?;
    }
    fts5_upsert(conn, id)?;
    Ok(())
}

pub fn delete_entry(conn: &mut Connection, id: &str) -> StorageResult<()> {
    if unified_schema_state(conn)? == UnifiedSchemaState::Complete {
        delete_entry_with_revision(conn, id)?;
        return Ok(());
    }

    let tx = conn.transaction()?;
    fts5_delete(&tx, id)?;
    let n = tx.execute("DELETE FROM vault_entries WHERE id=?1", params![id])?;
    tx.commit()?;
    if n == 0 {
        return Err(StorageError::Other(format!("entry not found: {id}")));
    }
    Ok(())
}

pub fn list_entries(
    conn: &Connection,
    kind: Option<EntryKind>,
) -> StorageResult<Vec<VaultEntrySummary>> {
    let entries: Vec<crate::vault::models::VaultEntry> = if let Some(k) = kind {
        let mut s = conn.prepare(
            "SELECT id, kind, title, notes, created_at, updated_at
             FROM vault_entries WHERE kind=?1 ORDER BY updated_at DESC",
        )?;
        let rows = s.query_map(params![k.as_str()], row_to_entry)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        let mut s = conn.prepare(
            "SELECT id, kind, title, notes, created_at, updated_at
             FROM vault_entries ORDER BY updated_at DESC",
        )?;
        let rows = s.query_map([], row_to_entry)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let mut summaries = Vec::with_capacity(entries.len());
    for entry in entries {
        let fields = list_fields(conn, &entry.id)?;
        let tags = list_tags_with_source(conn, &entry.id)?;
        let preview = build_preview(&entry, &fields);
        summaries.push(VaultEntrySummary {
            entry,
            tags,
            preview,
        });
    }
    Ok(summaries)
}

/// 生成预览：max 120 Unicode chars，绝不包含敏感字段值。
fn build_preview(
    entry: &crate::vault::models::VaultEntry,
    fields: &[VaultField],
) -> Option<String> {
    const MAX_LEN: usize = 120;
    let sensitive_values = fields
        .iter()
        .filter(|field| field.is_sensitive || is_default_sensitive_key(&field.key))
        .map(|field| field.value.clone())
        .collect::<Vec<_>>();

    let mut trim_to = |s: &str| -> Option<String> {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(unicode_truncate(t, MAX_LEN).to_string())
        }
    };

    let candidate: Option<String> = match entry.kind {
        EntryKind::Credential => {
            // 取前两个非敏感字段 value
            let non_sensitive: Vec<&VaultField> = fields
                .iter()
                .filter(|field| !field.is_sensitive && !is_default_sensitive_key(&field.key))
                .take(2)
                .collect();
            if non_sensitive.is_empty() {
                entry.notes.as_deref().and_then(&mut trim_to)
            } else {
                let joined = non_sensitive
                    .iter()
                    .map(|f| f.value.trim())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                if joined.is_empty() {
                    entry.notes.as_deref().and_then(&mut trim_to)
                } else {
                    trim_to(&joined)
                }
            }
        }
        EntryKind::Bookmark => {
            // 优先 URL，然后 notes
            let url = fields
                .iter()
                .find(|field| {
                    field.key.eq_ignore_ascii_case("url")
                        && !field.is_sensitive
                        && !is_default_sensitive_key(&field.key)
                })
                .map(|f| f.value.trim())
                .filter(|s| !s.is_empty());
            url.and_then(&mut trim_to)
                .or_else(|| entry.notes.as_deref().and_then(&mut trim_to))
        }
        EntryKind::Note => entry.notes.as_deref().and_then(&mut trim_to),
    };
    candidate
        .map(|preview| {
            crate::content::projection::redact_sensitive_values(&preview, &sensitive_values)
        })
        .filter(|preview| !preview.is_empty())
}

fn unicode_truncate(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let end = s
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    &s[..end]
}

pub fn get_entry_detail(conn: &Connection, id: &str) -> StorageResult<VaultEntryDetail> {
    let entry = get_entry_by_id(conn, id)?
        .ok_or_else(|| StorageError::Other(format!("entry not found: {id}")))?;
    let fields = list_fields(conn, id)?;
    let tags = list_tags_with_source(conn, id)?;
    let ai_metadata = get_ai_metadata(conn, id)?;
    Ok(VaultEntryDetail {
        entry,
        fields,
        tags,
        ai_metadata,
    })
}

pub fn set_manual_tags(
    conn: &mut Connection,
    entry_id: &str,
    tags: &[String],
) -> StorageResult<()> {
    let schema_state = unified_schema_state(conn)?;
    if normalized_tags_match(conn, entry_id, "manual", tags)? {
        return Ok(());
    }
    if schema_state == UnifiedSchemaState::Complete {
        set_manual_tags_with_revision(conn, entry_id, tags)?;
        return Ok(());
    }

    let tx = conn.transaction()?;
    write_manual_tags(&tx, entry_id, tags)?;
    fts5_upsert(&tx, entry_id)?;
    tx.commit()?;
    Ok(())
}

pub fn delete_entry_with_revision(
    conn: &mut Connection,
    id: &str,
) -> StorageResult<ContentMutation<()>> {
    require_unified_schema(conn)?;
    let tx = conn.transaction()?;
    let unified_id = vault_unified_id(id)?;
    fts5_delete(&tx, id)?;
    tx.execute(
        "DELETE FROM content_pending_deletes WHERE unified_id=?1",
        params![unified_id],
    )?;
    tx.execute(
        "DELETE FROM content_fts WHERE unified_id=?1",
        params![unified_id],
    )?;
    let catalog_rows = tx.execute(
        "DELETE FROM content_catalog WHERE unified_id=?1",
        params![unified_id],
    )?;
    if catalog_rows != 1 {
        return Err(StorageError::Validation(format!(
            "content catalog row not found: {unified_id}"
        )));
    }
    let payload_rows = tx.execute("DELETE FROM vault_entries WHERE id=?1", params![id])?;
    if payload_rows != 1 {
        return Err(StorageError::Other(format!("entry not found: {id}")));
    }
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    Ok(content_mutation(
        (),
        revision,
        unified_id,
        ContentOperation::Deleted,
    ))
}

pub fn set_manual_tags_with_revision(
    conn: &mut Connection,
    entry_id: &str,
    tags: &[String],
) -> StorageResult<ContentMutation<VaultEntryDetail>> {
    require_unified_schema(conn)?;
    let tx = conn.transaction()?;
    if normalized_tags_match(&tx, entry_id, "manual", tags)? {
        let detail = get_entry_detail(&tx, entry_id)?;
        let mutation = unchanged_content_mutation(&tx, detail)?;
        tx.commit()?;
        return Ok(mutation);
    }
    write_manual_tags(&tx, entry_id, tags)?;
    fts5_upsert(&tx, entry_id)?;
    let unified_id = sync_vault_content(&tx, entry_id)?;
    let revision = bump_revision(&tx)?;
    let detail = get_entry_detail(&tx, entry_id)?;
    tx.commit()?;
    Ok(content_mutation(
        detail,
        revision,
        unified_id,
        ContentOperation::Updated,
    ))
}

/// 事务内写入 manual tags：先清除该 entry 的所有 manual 行（不动 ai 行），
/// 然后插入新 manual tags。归一化失败（空/纯空白）的标签跳过。
fn write_manual_tags(conn: &Connection, entry_id: &str, tags: &[String]) -> StorageResult<()> {
    conn.execute(
        "DELETE FROM vault_tags WHERE entry_id=?1 AND source='manual'",
        params![entry_id],
    )?;
    for t in tags {
        if let Some(norm) = crate::vault::migrations::normalize_tag(t) {
            let display = t.trim().to_string();
            conn.execute(
                "INSERT OR IGNORE INTO vault_tags(entry_id, tag, normalized_tag, source)
                 VALUES (?1, ?2, ?3, 'manual')",
                params![entry_id, display, norm],
            )?;
        }
    }
    Ok(())
}

fn normalized_tags_match(
    conn: &Connection,
    entry_id: &str,
    source: &str,
    tags: &[String],
) -> StorageResult<bool> {
    if get_entry_by_id(conn, entry_id)?.is_none() {
        return Ok(false);
    }
    let mut expected = tags
        .iter()
        .filter_map(|tag| crate::vault::migrations::normalize_tag(tag))
        .collect::<Vec<_>>();
    expected.sort();
    expected.dedup();
    let stored = {
        let mut stmt = conn.prepare(
            "SELECT normalized_tag FROM vault_tags
             WHERE entry_id=?1 AND source=?2 ORDER BY normalized_tag",
        )?;
        let rows = stmt.query_map(params![entry_id, source], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    Ok(stored == expected)
}

/// 用新的 AI 标签集合替换该 entry 的所有 source='ai' 行（manual 不动）。
pub fn replace_ai_tags(
    conn: &mut Connection,
    entry_id: &str,
    tags: &[String],
) -> StorageResult<()> {
    let schema_state = unified_schema_state(conn)?;
    if normalized_tags_match(conn, entry_id, "ai", tags)? {
        return Ok(());
    }
    if schema_state == UnifiedSchemaState::Complete {
        replace_ai_tags_with_revision(conn, entry_id, tags)?;
        return Ok(());
    }

    let tx = conn.transaction()?;
    write_ai_tags(&tx, entry_id, tags)?;
    fts5_upsert(&tx, entry_id)?;
    tx.commit()?;
    Ok(())
}

pub fn replace_ai_tags_with_revision(
    conn: &mut Connection,
    entry_id: &str,
    tags: &[String],
) -> StorageResult<ContentMutation<()>> {
    require_unified_schema(conn)?;
    let tx = conn.transaction()?;
    if normalized_tags_match(&tx, entry_id, "ai", tags)? {
        let mutation = unchanged_content_mutation(&tx, ())?;
        tx.commit()?;
        return Ok(mutation);
    }
    write_ai_tags(&tx, entry_id, tags)?;
    fts5_upsert(&tx, entry_id)?;
    let unified_id = sync_vault_content(&tx, entry_id)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    Ok(content_mutation(
        (),
        revision,
        unified_id,
        ContentOperation::Updated,
    ))
}

fn write_ai_tags(conn: &Connection, entry_id: &str, tags: &[String]) -> StorageResult<()> {
    conn.execute(
        "DELETE FROM vault_tags WHERE entry_id=?1 AND source='ai'",
        params![entry_id],
    )?;
    for tag in tags {
        if let Some(normalized) = crate::vault::migrations::normalize_tag(tag) {
            conn.execute(
                "INSERT OR IGNORE INTO vault_tags(entry_id, tag, normalized_tag, source)
                 VALUES (?1, ?2, ?3, 'ai')",
                params![entry_id, tag.trim(), normalized],
            )?;
        }
    }
    Ok(())
}

/// 删除某个 normalized_tag 对应的 AI 行；同名 manual 行永远保留。
pub fn remove_ai_tag(
    conn: &mut Connection,
    entry_id: &str,
    normalized_tag: &str,
) -> StorageResult<()> {
    if unified_schema_state(conn)? == UnifiedSchemaState::Complete {
        remove_ai_tag_with_revision(conn, entry_id, normalized_tag)?;
        return Ok(());
    }

    let tx = conn.transaction()?;
    let affected = tx.execute(
        "DELETE FROM vault_tags
         WHERE entry_id=?1 AND source='ai' AND normalized_tag=?2",
        params![entry_id, normalized_tag],
    )?;
    if affected == 0 {
        get_entry_by_id(&tx, entry_id)?
            .ok_or_else(|| StorageError::Other(format!("entry not found: {entry_id}")))?;
        tx.commit()?;
        return Ok(());
    }
    fts5_upsert(&tx, entry_id)?;
    tx.commit()?;
    Ok(())
}

pub fn remove_ai_tag_with_revision(
    conn: &mut Connection,
    entry_id: &str,
    normalized_tag: &str,
) -> StorageResult<ContentMutation<VaultEntryDetail>> {
    require_unified_schema(conn)?;
    let tx = conn.transaction()?;
    let affected = tx.execute(
        "DELETE FROM vault_tags
         WHERE entry_id=?1 AND source='ai' AND normalized_tag=?2",
        params![entry_id, normalized_tag],
    )?;
    if affected == 0 {
        let detail = get_entry_detail(&tx, entry_id)?;
        let mutation = unchanged_content_mutation(&tx, detail)?;
        tx.commit()?;
        return Ok(mutation);
    }
    fts5_upsert(&tx, entry_id)?;
    let unified_id = sync_vault_content(&tx, entry_id)?;
    let revision = bump_revision(&tx)?;
    let detail = get_entry_detail(&tx, entry_id)?;
    tx.commit()?;
    Ok(content_mutation(
        detail,
        revision,
        unified_id,
        ContentOperation::Updated,
    ))
}

/// 写入完整的 ready AI metadata（status='ready'）。同事务刷新 FTS 以反映 search_aliases。
pub fn set_ai_metadata(conn: &mut Connection, metadata: &VaultAiMetadata) -> StorageResult<()> {
    let schema_state = unified_schema_state(conn)?;
    if ai_metadata_matches(conn, metadata)? {
        return Ok(());
    }
    if schema_state == UnifiedSchemaState::Complete {
        set_ai_metadata_with_revision(conn, metadata)?;
        return Ok(());
    }

    let tx = conn.transaction()?;
    write_ai_metadata(&tx, metadata)?;
    // 状态从 pending→ready 或 ready→pending 时刷新 FTS
    fts5_upsert(&tx, &metadata.entry_id)?;
    tx.commit()?;
    Ok(())
}

pub fn set_ai_metadata_with_revision(
    conn: &mut Connection,
    metadata: &VaultAiMetadata,
) -> StorageResult<ContentMutation<()>> {
    require_unified_schema(conn)?;
    let tx = conn.transaction()?;
    if ai_metadata_matches(&tx, metadata)? {
        let mutation = unchanged_content_mutation(&tx, ())?;
        tx.commit()?;
        return Ok(mutation);
    }
    write_ai_metadata(&tx, metadata)?;
    fts5_upsert(&tx, &metadata.entry_id)?;
    let unified_id = sync_vault_content(&tx, &metadata.entry_id)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    Ok(content_mutation(
        (),
        revision,
        unified_id,
        ContentOperation::Updated,
    ))
}

fn write_ai_metadata(conn: &Connection, metadata: &VaultAiMetadata) -> StorageResult<()> {
    let aliases_json = serde_json::to_string(&metadata.search_aliases)
        .map_err(|e| StorageError::Other(e.to_string()))?;
    conn.execute(
        "INSERT INTO vault_ai_metadata
            (entry_id, summary, search_aliases_json, content_hash,
             provider_id, model, generated_at, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(entry_id) DO UPDATE SET
            summary = excluded.summary,
            search_aliases_json = excluded.search_aliases_json,
            content_hash = excluded.content_hash,
            provider_id = excluded.provider_id,
            model = excluded.model,
            generated_at = excluded.generated_at,
            status = excluded.status",
        params![
            metadata.entry_id,
            metadata.summary,
            aliases_json,
            metadata.content_hash,
            metadata.provider_id,
            metadata.model,
            metadata.generated_at,
            metadata.status.as_str(),
        ],
    )?;
    Ok(())
}

fn ai_metadata_matches(conn: &Connection, expected: &VaultAiMetadata) -> StorageResult<bool> {
    let Some(stored) = get_ai_metadata(conn, &expected.entry_id)? else {
        return Ok(false);
    };
    Ok(stored.summary == expected.summary
        && stored.search_aliases == expected.search_aliases
        && stored.content_hash == expected.content_hash
        && stored.provider_id == expected.provider_id
        && stored.model == expected.model
        && stored.generated_at == expected.generated_at
        && stored.status == expected.status)
}

fn canonical_pending_matches(
    conn: &Connection,
    entry_id: &str,
    content_hash: &str,
) -> StorageResult<bool> {
    let Some(stored) = get_ai_metadata(conn, entry_id)? else {
        return Ok(false);
    };
    Ok(stored.status == AiMetadataStatus::Pending
        && stored.content_hash == content_hash
        && stored.summary.is_none()
        && stored.search_aliases.is_empty()
        && stored.provider_id.is_none()
        && stored.model.is_none()
        && stored.generated_at.is_none())
}

/// 把 metadata 置为 pending 状态（保留 entry_id，写入新 content_hash）。
/// 若该 entry 尚无 metadata 行，插入一条 pending 行。
pub fn mark_ai_metadata_pending(
    conn: &mut Connection,
    entry_id: &str,
    content_hash: &str,
) -> StorageResult<()> {
    let schema_state = unified_schema_state(conn)?;
    if canonical_pending_matches(conn, entry_id, content_hash)? {
        return Ok(());
    }
    if schema_state == UnifiedSchemaState::Complete {
        mark_ai_metadata_pending_with_revision(conn, entry_id, content_hash)?;
        return Ok(());
    }

    let tx = conn.transaction()?;
    upsert_ai_metadata_pending(&tx, entry_id, content_hash)?;
    fts5_upsert(&tx, entry_id)?;
    tx.commit()?;
    Ok(())
}

pub fn mark_ai_metadata_pending_with_revision(
    conn: &mut Connection,
    entry_id: &str,
    content_hash: &str,
) -> StorageResult<ContentMutation<()>> {
    require_unified_schema(conn)?;
    let tx = conn.transaction()?;
    if canonical_pending_matches(&tx, entry_id, content_hash)? {
        let mutation = unchanged_content_mutation(&tx, ())?;
        tx.commit()?;
        return Ok(mutation);
    }
    upsert_ai_metadata_pending(&tx, entry_id, content_hash)?;
    fts5_upsert(&tx, entry_id)?;
    let unified_id = sync_vault_content(&tx, entry_id)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    Ok(content_mutation(
        (),
        revision,
        unified_id,
        ContentOperation::Updated,
    ))
}

pub fn refresh_ai_metadata_with_revision(
    conn: &mut Connection,
    entry_id: &str,
) -> StorageResult<ContentMutation<()>> {
    require_unified_schema(conn)?;
    let tx = conn.transaction()?;
    let entry = get_entry_by_id(&tx, entry_id)?
        .ok_or_else(|| StorageError::Other(format!("entry not found: {entry_id}")))?;
    let fields = list_fields(&tx, entry_id)?;
    let content_hash = compute_entry_content_hash(&entry, &fields);
    if canonical_pending_matches(&tx, entry_id, &content_hash)? {
        let mutation = unchanged_content_mutation(&tx, ())?;
        tx.commit()?;
        return Ok(mutation);
    }
    upsert_ai_metadata_pending(&tx, entry_id, &content_hash)?;
    fts5_upsert(&tx, entry_id)?;
    let unified_id = sync_vault_content(&tx, entry_id)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    Ok(content_mutation(
        (),
        revision,
        unified_id,
        ContentOperation::Updated,
    ))
}

pub fn apply_ai_enrichment_if_pending(
    conn: &mut Connection,
    entry_id: &str,
    expected_hash: &str,
    ai_tags: &[String],
    metadata: &VaultAiMetadata,
) -> StorageResult<Option<ContentMutation<()>>> {
    require_unified_schema(conn)?;
    if metadata.entry_id != entry_id
        || metadata.content_hash != expected_hash
        || metadata.status != AiMetadataStatus::Ready
    {
        return Err(StorageError::Validation(
            "AI enrichment metadata does not match the pending snapshot".to_string(),
        ));
    }
    let tx = conn.transaction()?;
    if !pending_snapshot_matches(&tx, entry_id, expected_hash)? {
        return Ok(None);
    }
    write_ai_tags(&tx, entry_id, ai_tags)?;
    write_ai_metadata(&tx, metadata)?;
    fts5_upsert(&tx, entry_id)?;
    let unified_id = sync_vault_content(&tx, entry_id)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    Ok(Some(content_mutation(
        (),
        revision,
        unified_id,
        ContentOperation::Updated,
    )))
}

pub fn mark_ai_metadata_error_if_pending(
    conn: &mut Connection,
    entry_id: &str,
    expected_hash: &str,
) -> StorageResult<Option<ContentMutation<()>>> {
    require_unified_schema(conn)?;
    let tx = conn.transaction()?;
    if !pending_snapshot_matches(&tx, entry_id, expected_hash)? {
        return Ok(None);
    }
    let affected = tx.execute(
        "UPDATE vault_ai_metadata SET status='error'
         WHERE entry_id=?1 AND status='pending' AND content_hash=?2",
        params![entry_id, expected_hash],
    )?;
    if affected != 1 {
        return Ok(None);
    }
    fts5_upsert(&tx, entry_id)?;
    let unified_id = sync_vault_content(&tx, entry_id)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    Ok(Some(content_mutation(
        (),
        revision,
        unified_id,
        ContentOperation::Updated,
    )))
}

fn pending_snapshot_matches(
    conn: &Connection,
    entry_id: &str,
    expected_hash: &str,
) -> StorageResult<bool> {
    let metadata_matches: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM vault_ai_metadata
             WHERE entry_id=?1 AND status='pending' AND content_hash=?2
         )",
        params![entry_id, expected_hash],
        |row| row.get(0),
    )?;
    if !metadata_matches {
        return Ok(false);
    }
    let entry = match get_entry_by_id(conn, entry_id)? {
        Some(entry) => entry,
        None => return Ok(false),
    };
    let fields = list_fields(conn, entry_id)?;
    Ok(compute_entry_content_hash(&entry, &fields) == expected_hash)
}

/// 事务内的辅助：upsert 一条 pending metadata 行（summary=NULL，aliases=[]）。
fn upsert_ai_metadata_pending(
    conn: &Connection,
    entry_id: &str,
    content_hash: &str,
) -> StorageResult<()> {
    conn.execute(
        "INSERT INTO vault_ai_metadata
            (entry_id, summary, search_aliases_json, content_hash,
             provider_id, model, generated_at, status)
         VALUES (?1, NULL, '[]', ?2, NULL, NULL, NULL, 'pending')
         ON CONFLICT(entry_id) DO UPDATE SET
            summary = NULL,
            search_aliases_json = '[]',
            content_hash = excluded.content_hash,
            provider_id = NULL,
            model = NULL,
            generated_at = NULL,
            status = 'pending'",
        params![entry_id, content_hash],
    )?;
    Ok(())
}

/// 列出所有 metadata.status='pending' 的 entry_id，按 created_at 升序。
pub fn list_pending_ai_entries(conn: &Connection, limit: usize) -> StorageResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT m.entry_id
         FROM vault_ai_metadata m
         JOIN vault_entries e ON e.id = m.entry_id
         WHERE m.status = 'pending'
         ORDER BY e.created_at ASC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |r| r.get::<_, String>(0))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(StorageError::from)
}

/// 统计当前各 AI metadata 状态的条目数。
/// `processing` 字段始终为 0（worker 没有显式 processing 状态，pending 即"待处理"）。
pub fn backfill_status(conn: &Connection) -> StorageResult<BackfillStatus> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM vault_ai_metadata", [], |r| r.get(0))?;
    let ready: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vault_ai_metadata WHERE status='ready'",
        [],
        |r| r.get(0),
    )?;
    let pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vault_ai_metadata WHERE status='pending'",
        [],
        |r| r.get(0),
    )?;
    let error: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vault_ai_metadata WHERE status='error'",
        [],
        |r| r.get(0),
    )?;
    Ok(BackfillStatus {
        total: total as usize,
        pending: pending as usize,
        processing: 0,
        ready: ready as usize,
        error: error as usize,
    })
}

/// 读取某条目当前 AI 内容哈希（来自 metadata 行），若不存在返回空串。
pub fn ai_content_hash_for_entry(conn: &Connection, entry_id: &str) -> StorageResult<String> {
    let hash: Option<String> = conn
        .query_row(
            "SELECT content_hash FROM vault_ai_metadata WHERE entry_id=?1",
            params![entry_id],
            |r| r.get::<_, String>(0),
        )
        .ok();
    Ok(hash.unwrap_or_default())
}

/// 返回 entry 的所有标签（含 source 信息），按 tag 字典序排序。
pub fn list_tags(conn: &Connection, entry_id: &str) -> StorageResult<Vec<VaultTag>> {
    list_tags_with_source(conn, entry_id)
}

/// 返回带来源信息的标签列表（供 VaultEntryDetail 等结构化场景使用）。
pub fn list_tags_with_source(conn: &Connection, entry_id: &str) -> StorageResult<Vec<VaultTag>> {
    let mut stmt = conn.prepare(
        "SELECT tag, normalized_tag, source FROM vault_tags
         WHERE entry_id=?1 ORDER BY tag",
    )?;
    let rows = stmt.query_map(params![entry_id], |r| {
        let source_str: String = r.get(2)?;
        let source = match source_str.as_str() {
            "ai" => TagSource::Ai,
            _ => TagSource::Manual,
        };
        Ok(VaultTag {
            tag: r.get(0)?,
            normalized_tag: r.get(1)?,
            source,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(StorageError::from)
}

/// 读取一条条目的 AI 元数据；无记录时返回 None。
pub fn get_ai_metadata(
    conn: &Connection,
    entry_id: &str,
) -> StorageResult<Option<VaultAiMetadata>> {
    let mut stmt = conn.prepare(
        "SELECT entry_id, summary, search_aliases_json, content_hash,
                provider_id, model, generated_at, status
         FROM vault_ai_metadata WHERE entry_id=?1",
    )?;
    let mut rows = stmt.query(params![entry_id])?;
    let Some(r) = rows.next()? else {
        return Ok(None);
    };
    let aliases_json: String = r.get(2)?;
    let aliases: Vec<String> = serde_json::from_str(&aliases_json).unwrap_or_default();
    let status_str: String = r.get(7)?;
    let status = match status_str.as_str() {
        "pending" => AiMetadataStatus::Pending,
        "error" => AiMetadataStatus::Error,
        _ => AiMetadataStatus::Ready,
    };
    Ok(Some(VaultAiMetadata {
        entry_id: r.get(0)?,
        summary: r.get(1)?,
        search_aliases: aliases,
        content_hash: r.get(3)?,
        provider_id: r.get(4)?,
        model: r.get(5)?,
        generated_at: r.get(6)?,
        status,
    }))
}

fn build_searchable(conn: &Connection, entry_id: &str) -> StorageResult<String> {
    // 拼接所有非敏感字段的 value + 所有 tag
    let mut parts: Vec<String> = Vec::new();

    let mut stmt = conn.prepare(
        "SELECT key, value, is_sensitive FROM vault_fields
         WHERE entry_id=?1 ORDER BY sort_order ASC, id ASC",
    )?;
    let values = stmt.query_map(params![entry_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)? != 0,
        ))
    })?;
    for value in values {
        let (key, value, sensitive) = value?;
        if !sensitive && !is_default_sensitive_key(&key) {
            parts.push(value);
        }
    }

    let mut stmt = conn.prepare("SELECT tag FROM vault_tags WHERE entry_id=?1")?;
    let tags = stmt.query_map(params![entry_id], |r| r.get::<_, String>(0))?;
    for t in tags {
        parts.push(t?);
    }
    Ok(parts.join(" "))
}

fn fts5_upsert(conn: &Connection, entry_id: &str) -> StorageResult<()> {
    let entry = get_entry_by_id(conn, entry_id)?
        .ok_or_else(|| StorageError::Other(format!("entry {entry_id} missing for fts")))?;
    let searchable = build_searchable(conn, entry_id)?;
    let sensitive_values = sensitive_values(conn, entry_id)?;
    let title =
        crate::content::projection::redact_sensitive_values(&entry.title, &sensitive_values);
    let notes = crate::content::projection::redact_sensitive_values(
        entry.notes.as_deref().unwrap_or_default(),
        &sensitive_values,
    );
    let searchable =
        crate::content::projection::redact_sensitive_values(&searchable, &sensitive_values);
    conn.execute("DELETE FROM vault_fts WHERE entry_id=?1", params![entry_id])?;
    conn.execute(
        "INSERT INTO vault_fts(entry_id, title, notes, searchable) VALUES (?1, ?2, ?3, ?4)",
        params![entry_id, title, notes, searchable],
    )?;
    Ok(())
}

/// Rebuilds the legacy Vault FTS table through the same privacy projection used by mutations.
pub(crate) fn rebuild_vault_fts(conn: &Connection) -> StorageResult<()> {
    let entry_ids = {
        let mut stmt = conn.prepare("SELECT id FROM vault_entries ORDER BY created_at, id")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    conn.execute("DELETE FROM vault_fts", [])?;
    for entry_id in entry_ids {
        fts5_upsert(conn, &entry_id)?;
    }
    Ok(())
}

fn sensitive_values(conn: &Connection, entry_id: &str) -> StorageResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT key, value, is_sensitive FROM vault_fields
         WHERE entry_id=?1
         ORDER BY sort_order ASC, id ASC",
    )?;
    let values = stmt
        .query_map(params![entry_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? != 0,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values
        .into_iter()
        .filter_map(|(key, value, sensitive)| {
            (sensitive || is_default_sensitive_key(&key)).then_some(value)
        })
        .collect())
}

fn fts5_delete(conn: &Connection, entry_id: &str) -> StorageResult<()> {
    conn.execute("DELETE FROM vault_fts WHERE entry_id=?1", params![entry_id])?;
    Ok(())
}

pub fn fts5_search(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> StorageResult<Vec<(String, f64)>> {
    // SQLite FTS5 BM25：rank 越小越相关（升序排）。返回值 score 沿用 rank 语义。
    let sql = format!(
        "SELECT entry_id, rank FROM vault_fts WHERE vault_fts MATCH ?1
         ORDER BY rank LIMIT {limit}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![escape_fts_query(query)], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(StorageError::from)
}

fn escape_fts_query(q: &str) -> String {
    // 按空格分词，每个 token 单独转义后用 OR 连接（FTS5 默认 token 之间是隐式 AND，
    // 多词查询用 OR 才能召回更广；中文 unicode61 分词器会按字分词，单 token 也可行）
    let tokens: Vec<String> = q
        .split_whitespace()
        .map(|tok| {
            let escaped = tok.replace('"', "\"\"");
            format!("\"{escaped}\"")
        })
        .collect();
    if tokens.is_empty() {
        // 全空格 / 空字符串：返回不可能匹配的查询
        "\"\"".to_string()
    } else {
        tokens.join(" OR ")
    }
}

/// AI 内容哈希的 canonical 输入：
///   {kind}\n{trimmed_title}\n{trimmed_notes}\n{joined_fields}
/// joined_fields 每行是 `{lowercased_key}={value}`；敏感字段的 value 恒为 `<sensitive>`，
/// 因此密码轮换不会让哈希变化。
fn ai_content_hash(input: &VaultEntryInput) -> String {
    let fields = input
        .fields
        .iter()
        .map(|f| {
            let value = if f.is_sensitive || is_default_sensitive_key(&f.key) {
                "<sensitive>".to_string()
            } else {
                f.value.trim().to_string()
            };
            format!("{}={value}", f.key.trim().to_lowercase())
        })
        .collect::<Vec<_>>()
        .join("\n");
    let canonical = format!(
        "{}\n{}\n{}\n{}",
        input.kind.as_str(),
        input.title.trim(),
        input.notes.as_deref().unwrap_or("").trim(),
        fields,
    );
    hex::encode(Sha256::digest(canonical.as_bytes()))
}

/// 从存储的 entry + fields 重新计算当前 content hash。
/// Task 9 search 用它判断 metadata.content_hash 是否已经 stale。
/// 与 `ai_content_hash(input)` 的 canonical 输入完全一致，因此可以
/// 直接拿来和 metadata.content_hash 对比。
pub fn compute_entry_content_hash(entry: &VaultEntry, fields: &[VaultField]) -> String {
    let fields_str = fields
        .iter()
        .map(|f| {
            let value = if f.is_sensitive || is_default_sensitive_key(&f.key) {
                "<sensitive>".to_string()
            } else {
                f.value.trim().to_string()
            };
            format!("{}={value}", f.key.trim().to_lowercase())
        })
        .collect::<Vec<_>>()
        .join("\n");
    let canonical = format!(
        "{}\n{}\n{}\n{}",
        entry.kind.as_str(),
        entry.title.trim(),
        entry.notes.as_deref().unwrap_or("").trim(),
        fields_str,
    );
    hex::encode(Sha256::digest(canonical.as_bytes()))
}

/// 从 capture draft 原子地创建 entry：
///  - 若 request_id 已存在于 vault_capture_requests，直接返回对应 entry（idempotent）
///  - 否则在单事务内写入：entry + fields + manual_tags + ai_tags + ready metadata + request_id + FTS
pub fn create_from_capture(
    conn: &mut Connection,
    draft: &CaptureDraft,
    request_id: &str,
) -> StorageResult<VaultEntryDetail> {
    if unified_schema_state(conn)? == UnifiedSchemaState::Complete {
        return Ok(create_from_capture_with_revision(conn, draft, request_id)?.value);
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(id) = capture_entry_id(&tx, request_id)? {
        tx.commit()?;
        return get_entry_detail(conn, &id);
    }
    let id = insert_capture_entry(&tx, draft, request_id)?;
    fts5_upsert(&tx, &id)?;
    tx.commit()?;
    get_entry_detail(conn, &id)
}

pub fn create_from_capture_with_revision(
    conn: &mut Connection,
    draft: &CaptureDraft,
    request_id: &str,
) -> StorageResult<ContentMutation<VaultEntryDetail>> {
    require_unified_schema(conn)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(id) = capture_entry_id(&tx, request_id)? {
        let revision = crate::content::catalog::current_revision(&tx)?;
        let detail = get_entry_detail(&tx, &id)?;
        tx.commit()?;
        return Ok(ContentMutation {
            value: detail,
            revision,
            changes: Vec::new(),
        });
    }
    let id = insert_capture_entry(&tx, draft, request_id)?;
    fts5_upsert(&tx, &id)?;
    let unified_id = sync_vault_content(&tx, &id)?;
    let revision = bump_revision(&tx)?;
    let detail = get_entry_detail(&tx, &id)?;
    tx.commit()?;
    Ok(content_mutation(
        detail,
        revision,
        unified_id,
        ContentOperation::Created,
    ))
}

fn capture_entry_id(conn: &Connection, request_id: &str) -> StorageResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT entry_id FROM vault_capture_requests WHERE request_id=?1",
            params![request_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn insert_capture_entry(
    conn: &Connection,
    draft: &CaptureDraft,
    request_id: &str,
) -> StorageResult<String> {
    let id = next_entry_id();
    let now = now_rfc3339();

    conn.execute(
        "INSERT INTO vault_entries(id, kind, title, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, draft.kind.as_str(), draft.title, draft.notes, now, now],
    )?;

    for (i, f) in draft.fields.iter().enumerate() {
        let sensitive = f.is_sensitive || is_default_sensitive_key(&f.key);
        let fid = next_field_id();
        conn.execute(
            "INSERT INTO vault_fields(id, entry_id, key, value, is_sensitive, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![fid, id, f.key, f.value, sensitive as i32, i as i64],
        )?;
    }

    // 写入手动标签
    write_manual_tags(conn, &id, &draft.manual_tags)?;

    // 写入 AI 标签（source='ai'）
    for t in &draft.ai_tags {
        if let Some(norm) = crate::vault::migrations::normalize_tag(t) {
            let display = t.trim().to_string();
            conn.execute(
                "INSERT OR IGNORE INTO vault_tags(entry_id, tag, normalized_tag, source)
                 VALUES (?1, ?2, ?3, 'ai')",
                params![id, display, norm],
            )?;
        }
    }

    // 写入 metadata（status 取决于 draft 是否携带合法 AI provenance + 至少一个 AI 内容字段）。
    // - 若有 provenance AND 有 summary/aliases/tags → 直接 ready，不再调 LLM。
    // - 否则 → pending，留待 backfill worker 拾起。
    let content_hash = compute_content_hash_for_capture(draft);
    let aliases_json = serde_json::to_string(&draft.search_aliases)
        .map_err(|e| StorageError::Other(e.to_string()))?;
    let (provider_id, model, generated_at) = match &draft.ai_provenance {
        Some(p) => (
            Some(p.provider_id.clone()),
            Some(p.model.clone()),
            Some(p.generated_at.clone()),
        ),
        None => (None, None, None),
    };
    let has_ai_content = draft
        .ai_summary
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty())
        || !draft.search_aliases.is_empty()
        || !draft.ai_tags.is_empty();
    let status_value = if draft.ai_provenance.is_some() && has_ai_content {
        "ready"
    } else {
        "pending"
    };
    conn.execute(
        "INSERT INTO vault_ai_metadata
            (entry_id, summary, search_aliases_json, content_hash,
             provider_id, model, generated_at, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            draft.ai_summary,
            aliases_json,
            content_hash,
            provider_id,
            model,
            generated_at,
            status_value
        ],
    )?;

    // 记录 request_id 用于幂等
    conn.execute(
        "INSERT INTO vault_capture_requests(request_id, entry_id, created_at)
         VALUES (?1, ?2, ?3)",
        params![request_id, id, now],
    )?;
    Ok(id)
}

/// 用与 VaultEntryInput 相同的 canonical 算法计算 capture draft 的内容哈希。
fn compute_content_hash_for_capture(draft: &CaptureDraft) -> String {
    let fields = draft
        .fields
        .iter()
        .map(|f| {
            let value = if f.is_sensitive || is_default_sensitive_key(&f.key) {
                "<sensitive>".to_string()
            } else {
                f.value.trim().to_string()
            };
            format!("{}={value}", f.key.trim().to_lowercase())
        })
        .collect::<Vec<_>>()
        .join("\n");
    let canonical = format!(
        "{}\n{}\n{}\n{}",
        draft.kind.as_str(),
        draft.title.trim(),
        draft.notes.as_deref().unwrap_or("").trim(),
        fields,
    );
    hex::encode(Sha256::digest(canonical.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::models::{
        AiMetadataStatus, AiProvenance, CaptureDraft, CaptureField, FieldInput, VaultEntry,
    };

    fn open_test_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        ensure_vault_schema(&mut conn).unwrap();
        conn
    }

    fn open_unified_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::scratchpad::storage::ensure_dock_schema(&mut conn).unwrap();
        ensure_vault_schema(&mut conn).unwrap();
        crate::content::migrations::ensure_content_schema(&mut conn, 7).unwrap();
        conn
    }

    #[test]
    fn vault_writes_refresh_unified_projection_without_sensitive_fields() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "Server".into(),
                fields: vec![
                    FieldInput {
                        key: "username".into(),
                        value: "alice".into(),
                        is_sensitive: false,
                    },
                    FieldInput {
                        key: "password".into(),
                        value: "NeverIndexMe".into(),
                        is_sensitive: false,
                    },
                ],
                notes: None,
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        let unified_id = format!("vault:{}", detail.entry.id);

        let catalog: (String, Option<String>, i64, i64) = conn
            .query_row(
                "SELECT retention_state, cleanup_at,
                        (SELECT COUNT(*) FROM content_catalog WHERE unified_id=?1),
                        (SELECT COUNT(*) FROM content_fts WHERE unified_id=?1)
                 FROM content_catalog WHERE unified_id=?1",
                params![unified_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(catalog, ("saved".into(), None, 1, 1));

        let document =
            crate::content::projection::build_search_document(&conn, &unified_id).unwrap();
        assert!(document.body.contains("alice"));
        for safe_text in [
            document.title,
            document.body,
            document.tags,
            document.aliases,
        ] {
            assert!(!safe_text.contains("NeverIndexMe"));
        }

        set_manual_tags(&mut conn, &detail.entry.id, &["production".into()]).unwrap();
        let tags: String = conn
            .query_row(
                "SELECT tags FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(tags.contains("production"));
        let revision: i64 = conn
            .query_row(
                "SELECT revision FROM content_state WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revision, 2);
    }

    #[test]
    fn ai_metadata_write_refreshes_aliases_without_exposing_sensitive_values() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "Server".into(),
                fields: vec![
                    FieldInput {
                        key: "username".into(),
                        value: "alice".into(),
                        is_sensitive: false,
                    },
                    FieldInput {
                        key: "password".into(),
                        value: "NeverIndexMe".into(),
                        is_sensitive: true,
                    },
                ],
                notes: None,
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        let unified_id = format!("vault:{}", detail.entry.id);
        let content_hash = ai_content_hash_for_entry(&conn, &detail.entry.id).unwrap();

        set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: detail.entry.id,
                summary: Some("production login".into()),
                search_aliases: vec!["prod console".into()],
                content_hash,
                provider_id: Some("provider".into()),
                model: Some("model".into()),
                generated_at: Some("2026-07-18T00:00:00Z".into()),
                status: AiMetadataStatus::Ready,
            },
        )
        .unwrap();

        let (title, body, tags, aliases): (String, String, String, String) = conn
            .query_row(
                "SELECT title, body, tags, aliases FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert!(aliases.contains("prod console"));
        for safe_text in [title, body, tags, aliases] {
            assert!(!safe_text.contains("NeverIndexMe"));
        }
    }

    #[test]
    fn update_entry_refreshes_unified_projection_without_resetting_saved_membership() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "Old Server".into(),
                fields: Vec::new(),
                notes: None,
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        let unified_id = format!("vault:{}", detail.entry.id);
        let original_position: f64 = conn
            .query_row(
                "SELECT saved_position FROM content_catalog WHERE unified_id=?1",
                params![unified_id],
                |row| row.get(0),
            )
            .unwrap();

        update_entry(
            &mut conn,
            &detail.entry.id,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "New Server".into(),
                fields: vec![FieldInput {
                    key: "username".into(),
                    value: "new-alice".into(),
                    is_sensitive: false,
                }],
                notes: Some("new notes".into()),
                manual_tags: vec!["updated".into()],
            },
        )
        .unwrap();

        let (retention, cleanup_at, saved_position): (String, Option<String>, f64) = conn
            .query_row(
                "SELECT retention_state, cleanup_at, saved_position
                 FROM content_catalog WHERE unified_id=?1",
                params![unified_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(retention, "saved");
        assert_eq!(cleanup_at, None);
        assert_eq!(saved_position, original_position);
        let (title, body, tags): (String, String, String) = conn
            .query_row(
                "SELECT title, body, tags FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(title, "New Server");
        assert!(body.contains("new-alice"));
        assert!(body.contains("new notes"));
        assert!(tags.contains("updated"));
        assert_eq!(crate::content::catalog::current_revision(&conn).unwrap(), 2);
    }

    #[test]
    fn ai_tag_mutations_refresh_unified_tags_and_bump_once_each() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Note,
                title: "Console".into(),
                fields: Vec::new(),
                notes: None,
                manual_tags: vec!["manual".into()],
            },
        )
        .unwrap();
        let unified_id = format!("vault:{}", detail.entry.id);

        replace_ai_tags(
            &mut conn,
            &detail.entry.id,
            &["ai-one".into(), "ai-two".into()],
        )
        .unwrap();
        let tags: String = conn
            .query_row(
                "SELECT tags FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(tags.contains("manual"));
        assert!(tags.contains("ai-one"));
        assert!(tags.contains("ai-two"));
        assert_eq!(crate::content::catalog::current_revision(&conn).unwrap(), 2);

        remove_ai_tag(&mut conn, &detail.entry.id, "ai-one").unwrap();
        let tags: String = conn
            .query_row(
                "SELECT tags FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!tags.contains("ai-one"));
        assert!(tags.contains("ai-two"));
        assert_eq!(crate::content::catalog::current_revision(&conn).unwrap(), 3);
    }

    #[test]
    fn mark_pending_clears_ready_projection_and_bumps_once() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Note,
                title: "Console".into(),
                fields: Vec::new(),
                notes: None,
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        let unified_id = format!("vault:{}", detail.entry.id);
        let hash = ai_content_hash_for_entry(&conn, &detail.entry.id).unwrap();
        set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: detail.entry.id.clone(),
                summary: Some("ready summary".into()),
                search_aliases: vec!["ready alias".into()],
                content_hash: hash.clone(),
                provider_id: None,
                model: None,
                generated_at: None,
                status: AiMetadataStatus::Ready,
            },
        )
        .unwrap();

        mark_ai_metadata_pending(&mut conn, &detail.entry.id, &hash).unwrap();
        let (body, aliases): (String, String) = conn
            .query_row(
                "SELECT body, aliases FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(!body.contains("ready summary"));
        assert!(!aliases.contains("ready alias"));
        assert_eq!(crate::content::catalog::current_revision(&conn).unwrap(), 3);
    }

    #[test]
    fn delete_entry_removes_unified_rows_and_bumps_once() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Note,
                title: "Delete".into(),
                fields: Vec::new(),
                notes: None,
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        let unified_id = format!("vault:{}", detail.entry.id);

        delete_entry(&mut conn, &detail.entry.id).unwrap();

        for table in ["content_catalog", "content_fts"] {
            let count: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE unified_id=?1"),
                    params![unified_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "{table} row must be deleted");
        }
        assert_eq!(crate::content::catalog::current_revision(&conn).unwrap(), 2);
    }

    #[test]
    fn capture_retry_keeps_one_projection_and_does_not_bump_again() {
        let mut conn = open_unified_db();
        let draft = make_capture_draft("Captured");

        let first = create_from_capture(&mut conn, &draft, "capture-once").unwrap();
        let first_revision = crate::content::catalog::current_revision(&conn).unwrap();
        let second = create_from_capture(&mut conn, &draft, "capture-once").unwrap();

        assert_eq!(first.entry.id, second.entry.id);
        assert_eq!(first_revision, 1);
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            first_revision
        );
        let unified_id = format!("vault:{}", first.entry.id);
        for table in ["content_catalog", "content_fts"] {
            let count: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE unified_id=?1"),
                    params![unified_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "{table} must contain one row");
        }
    }

    #[test]
    fn unified_sync_failure_rolls_back_every_vault_update_and_revision() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "Before".into(),
                fields: vec![FieldInput {
                    key: "username".into(),
                    value: "before-user".into(),
                    is_sensitive: false,
                }],
                notes: Some("before notes".into()),
                manual_tags: vec!["before-manual".into()],
            },
        )
        .unwrap();
        let id = detail.entry.id;
        replace_ai_tags(&mut conn, &id, &["before-ai".into()]).unwrap();
        let hash = ai_content_hash_for_entry(&conn, &id).unwrap();
        set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: id.clone(),
                summary: Some("before summary".into()),
                search_aliases: vec!["before alias".into()],
                content_hash: hash,
                provider_id: Some("provider".into()),
                model: Some("model".into()),
                generated_at: Some("2026-07-18T00:00:00Z".into()),
                status: AiMetadataStatus::Ready,
            },
        )
        .unwrap();
        let unified_id = format!("vault:{id}");
        let before_detail = serde_json::to_value(get_entry_detail(&conn, &id).unwrap()).unwrap();
        let before_legacy_fts: (String, String, String) = conn
            .query_row(
                "SELECT title, notes, searchable FROM vault_fts WHERE entry_id=?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let before_projection: (String, String, String, String) = conn
            .query_row(
                "SELECT title, body, tags, aliases FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        let before_revision = crate::content::catalog::current_revision(&conn).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_vault_catalog_update
             BEFORE UPDATE ON content_catalog
             WHEN OLD.unified_id = 'vault:' || OLD.source_id
             BEGIN SELECT RAISE(FAIL, 'forced catalog sync failure'); END;",
        )
        .unwrap();

        let result = update_entry(
            &mut conn,
            &id,
            &VaultEntryInput {
                kind: EntryKind::Bookmark,
                title: "After".into(),
                fields: vec![FieldInput {
                    key: "url".into(),
                    value: "https://after.invalid".into(),
                    is_sensitive: false,
                }],
                notes: Some("after notes".into()),
                manual_tags: vec!["after-manual".into()],
            },
        );

        assert!(result.is_err());
        assert_eq!(
            serde_json::to_value(get_entry_detail(&conn, &id).unwrap()).unwrap(),
            before_detail
        );
        assert_eq!(
            conn.query_row(
                "SELECT title, notes, searchable FROM vault_fts WHERE entry_id=?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap(),
            before_legacy_fts
        );
        assert_eq!(
            conn.query_row(
                "SELECT title, body, tags, aliases FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap(),
            before_projection
        );
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            before_revision
        );
    }

    #[test]
    fn unified_delete_failure_preserves_vault_catalog_projection_and_revision() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Note,
                title: "Keep".into(),
                fields: Vec::new(),
                notes: Some("keep body".into()),
                manual_tags: vec!["keep-tag".into()],
            },
        )
        .unwrap();
        let id = detail.entry.id;
        let unified_id = format!("vault:{id}");
        let before_revision = crate::content::catalog::current_revision(&conn).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_vault_catalog_delete
             BEFORE DELETE ON content_catalog
             WHEN OLD.unified_id = 'vault:' || OLD.source_id
             BEGIN SELECT RAISE(FAIL, 'forced catalog delete failure'); END;",
        )
        .unwrap();

        assert!(delete_entry(&mut conn, &id).is_err());

        assert!(get_entry_by_id(&conn, &id).unwrap().is_some());
        assert_eq!(list_tags(&conn, &id).unwrap().len(), 1);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM vault_fts WHERE entry_id=?1",
                params![id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        for table in ["content_catalog", "content_fts"] {
            assert_eq!(
                conn.query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE unified_id=?1"),
                    params![unified_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
                1
            );
        }
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            before_revision
        );
    }

    #[test]
    fn sensitive_field_values_are_redacted_from_every_unified_projection_column() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "NeverIndexMe Server".into(),
                fields: vec![
                    FieldInput {
                        key: "username".into(),
                        value: "alice".into(),
                        is_sensitive: false,
                    },
                    FieldInput {
                        key: "password".into(),
                        value: "NeverIndexMe".into(),
                        is_sensitive: true,
                    },
                ],
                notes: Some("NeverIndexMe production notes".into()),
                manual_tags: vec!["NeverIndexMe-tag".into(), "production".into()],
            },
        )
        .unwrap();
        let hash = ai_content_hash_for_entry(&conn, &detail.entry.id).unwrap();
        set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: detail.entry.id.clone(),
                summary: Some("NeverIndexMe production summary".into()),
                search_aliases: vec!["NeverIndexMe alias".into(), "prod console".into()],
                content_hash: hash,
                provider_id: Some("provider-key-must-not-project".into()),
                model: Some("model".into()),
                generated_at: Some("2026-07-18T00:00:00Z".into()),
                status: AiMetadataStatus::Ready,
            },
        )
        .unwrap();
        let unified_id = format!("vault:{}", detail.entry.id);
        let columns: (String, String, String, String) = conn
            .query_row(
                "SELECT title, body, tags, aliases FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();

        for safe_text in [&columns.0, &columns.1, &columns.2, &columns.3] {
            assert!(!safe_text.contains("NeverIndexMe"));
            assert!(!safe_text.contains("provider-key-must-not-project"));
        }
        assert!(columns.1.contains("alice"));
        assert!(columns.2.contains("production"));
        assert!(columns.3.contains("prod console"));

        let legacy_columns: (String, String, String) = conn
            .query_row(
                "SELECT title, notes, searchable FROM vault_fts WHERE entry_id=?1",
                params![detail.entry.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        for safe_text in [&legacy_columns.0, &legacy_columns.1, &legacy_columns.2] {
            assert!(!safe_text.contains("NeverIndexMe"));
        }
        assert!(legacy_columns.2.contains("alice"));
        assert!(legacy_columns.2.contains("production"));
    }

    #[test]
    fn sensitive_case_variants_never_reach_either_fts_index() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "NEVERINDEXME Server".into(),
                fields: vec![
                    FieldInput {
                        key: "username".into(),
                        value: "alice".into(),
                        is_sensitive: false,
                    },
                    FieldInput {
                        key: "password".into(),
                        value: "NeverIndexMe".into(),
                        is_sensitive: true,
                    },
                ],
                notes: Some("neverindexme production notes".into()),
                manual_tags: vec!["NeVeRiNdExMe-tag".into(), "production".into()],
            },
        )
        .unwrap();
        let hash = ai_content_hash_for_entry(&conn, &detail.entry.id).unwrap();
        set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: detail.entry.id.clone(),
                summary: Some("summary neverindexme".into()),
                search_aliases: vec!["NEVERINDEXME alias".into(), "prod console".into()],
                content_hash: hash,
                provider_id: None,
                model: None,
                generated_at: None,
                status: AiMetadataStatus::Ready,
            },
        )
        .unwrap();

        let unified_text: String = conn
            .query_row(
                "SELECT title || ' ' || body || ' ' || tags || ' ' || aliases
                 FROM content_fts WHERE unified_id=?1",
                params![format!("vault:{}", detail.entry.id)],
                |row| row.get(0),
            )
            .unwrap();
        let legacy_text: String = conn
            .query_row(
                "SELECT title || ' ' || notes || ' ' || searchable
                 FROM vault_fts WHERE entry_id=?1",
                params![detail.entry.id],
                |row| row.get(0),
            )
            .unwrap();
        for indexed in [&unified_text, &legacy_text] {
            assert!(!indexed.to_lowercase().contains("neverindexme"));
            assert!(indexed.contains("alice"));
            assert!(indexed.contains("production"));
        }
        let vault_matches: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vault_fts WHERE vault_fts MATCH 'neverindexme'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let content_matches: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM content_fts WHERE content_fts MATCH 'neverindexme'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(vault_matches, 0);
        assert_eq!(content_matches, 0);
    }

    #[test]
    fn ready_ai_enrichment_applies_tags_metadata_projection_and_one_revision_atomically() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Note,
                title: "Pending".into(),
                fields: Vec::new(),
                notes: Some("body".into()),
                manual_tags: vec!["manual".into()],
            },
        )
        .unwrap();
        let id = detail.entry.id;
        let hash = ai_content_hash_for_entry(&conn, &id).unwrap();
        let metadata = VaultAiMetadata {
            entry_id: id.clone(),
            summary: Some("ready summary".into()),
            search_aliases: vec!["ready alias".into()],
            content_hash: hash.clone(),
            provider_id: Some("provider".into()),
            model: Some("model".into()),
            generated_at: Some("2026-07-18T00:00:00Z".into()),
            status: AiMetadataStatus::Ready,
        };

        let mutation =
            apply_ai_enrichment_if_pending(&mut conn, &id, &hash, &["ai-ready".into()], &metadata)
                .unwrap()
                .expect("pending snapshot must be applied");

        assert_eq!(mutation.revision, 2);
        assert_eq!(mutation.changes[0].id, format!("vault:{id}"));
        assert_eq!(mutation.changes[0].operation, ContentOperation::Updated);
        let written = get_ai_metadata(&conn, &id).unwrap().unwrap();
        assert_eq!(written.status, AiMetadataStatus::Ready);
        assert_eq!(written.search_aliases, vec!["ready alias"]);
        assert!(list_tags(&conn, &id)
            .unwrap()
            .iter()
            .any(|tag| tag.tag == "ai-ready" && tag.source == TagSource::Ai));
        let (body, aliases): (String, String) = conn
            .query_row(
                "SELECT body, aliases FROM content_fts WHERE unified_id=?1",
                params![format!("vault:{id}")],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(body.contains("ready summary"));
        assert!(aliases.contains("ready alias"));

        let stale_metadata = VaultAiMetadata {
            content_hash: "stale-hash".into(),
            ..metadata.clone()
        };
        assert!(apply_ai_enrichment_if_pending(
            &mut conn,
            &id,
            "stale-hash",
            &["stale-tag".into()],
            &stale_metadata,
        )
        .unwrap()
        .is_none());
        assert_eq!(crate::content::catalog::current_revision(&conn).unwrap(), 2);
        assert!(!list_tags(&conn, &id)
            .unwrap()
            .iter()
            .any(|tag| tag.tag == "stale-tag"));
    }

    #[test]
    fn failed_ready_ai_enrichment_rolls_back_tags_metadata_fts_and_revision() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Note,
                title: "Pending".into(),
                fields: Vec::new(),
                notes: Some("body".into()),
                manual_tags: vec!["manual".into()],
            },
        )
        .unwrap();
        let id = detail.entry.id;
        let hash = ai_content_hash_for_entry(&conn, &id).unwrap();
        let before_detail = serde_json::to_value(get_entry_detail(&conn, &id).unwrap()).unwrap();
        let before_legacy_fts: (String, String, String) = conn
            .query_row(
                "SELECT title, notes, searchable FROM vault_fts WHERE entry_id=?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let before_revision = crate::content::catalog::current_revision(&conn).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_ready_catalog_update
             BEFORE UPDATE ON content_catalog
             BEGIN SELECT RAISE(FAIL, 'forced ready sync failure'); END;",
        )
        .unwrap();
        let metadata = VaultAiMetadata {
            entry_id: id.clone(),
            summary: Some("must rollback".into()),
            search_aliases: vec!["must rollback".into()],
            content_hash: hash.clone(),
            provider_id: Some("provider".into()),
            model: Some("model".into()),
            generated_at: Some("2026-07-18T00:00:00Z".into()),
            status: AiMetadataStatus::Ready,
        };

        assert!(apply_ai_enrichment_if_pending(
            &mut conn,
            &id,
            &hash,
            &["must-rollback".into()],
            &metadata,
        )
        .is_err());

        assert_eq!(
            serde_json::to_value(get_entry_detail(&conn, &id).unwrap()).unwrap(),
            before_detail
        );
        assert_eq!(
            conn.query_row(
                "SELECT title, notes, searchable FROM vault_fts WHERE entry_id=?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap(),
            before_legacy_fts
        );
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            before_revision
        );
    }

    #[test]
    fn metadata_error_only_updates_the_matching_pending_snapshot_once() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Note,
                title: "Pending".into(),
                fields: Vec::new(),
                notes: None,
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        let id = detail.entry.id;
        let hash = ai_content_hash_for_entry(&conn, &id).unwrap();

        assert!(
            mark_ai_metadata_error_if_pending(&mut conn, &id, "stale-hash")
                .unwrap()
                .is_none()
        );
        assert_eq!(crate::content::catalog::current_revision(&conn).unwrap(), 1);

        let mutation = mark_ai_metadata_error_if_pending(&mut conn, &id, &hash)
            .unwrap()
            .expect("matching pending snapshot must transition to error");
        assert_eq!(mutation.revision, 2);
        assert_eq!(
            get_ai_metadata(&conn, &id).unwrap().unwrap().status,
            AiMetadataStatus::Error
        );
        assert!(mark_ai_metadata_error_if_pending(&mut conn, &id, &hash)
            .unwrap()
            .is_none());
        assert_eq!(crate::content::catalog::current_revision(&conn).unwrap(), 2);
    }

    #[test]
    fn unified_create_and_capture_failures_leave_no_partial_rows_or_revision() {
        let mut conn = open_unified_db();
        conn.execute_batch(
            "CREATE TRIGGER fail_new_vault_catalog
             BEFORE INSERT ON content_catalog
             WHEN NEW.source='vault'
             BEGIN SELECT RAISE(FAIL, 'forced create sync failure'); END;",
        )
        .unwrap();
        let input = VaultEntryInput {
            kind: EntryKind::Credential,
            title: "Must Roll Back".into(),
            fields: vec![FieldInput {
                key: "username".into(),
                value: "alice".into(),
                is_sensitive: false,
            }],
            notes: None,
            manual_tags: vec!["manual".into()],
        };

        assert!(create_entry(&mut conn, &input).is_err());
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM vault_entries WHERE title='Must Roll Back'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
        for table in [
            "vault_fields",
            "vault_tags",
            "vault_ai_metadata",
            "vault_fts",
            "content_catalog",
            "content_fts",
        ] {
            assert_eq!(
                conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
                0,
                "{table} must remain empty"
            );
        }
        assert_eq!(crate::content::catalog::current_revision(&conn).unwrap(), 0);

        let draft = make_capture_draft("Capture Must Roll Back");
        assert!(create_from_capture(&mut conn, &draft, "capture-failed-sync").is_err());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM vault_capture_requests", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        assert_eq!(crate::content::catalog::current_revision(&conn).unwrap(), 0);
    }

    #[test]
    fn pending_and_error_projection_failures_roll_back_metadata_and_revision() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Note,
                title: "Metadata".into(),
                fields: Vec::new(),
                notes: None,
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        let id = detail.entry.id;
        let hash = ai_content_hash_for_entry(&conn, &id).unwrap();
        set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: id.clone(),
                summary: Some("ready summary".into()),
                search_aliases: vec!["ready alias".into()],
                content_hash: hash.clone(),
                provider_id: None,
                model: None,
                generated_at: None,
                status: AiMetadataStatus::Ready,
            },
        )
        .unwrap();
        let before_revision = crate::content::catalog::current_revision(&conn).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_metadata_catalog_update
             BEFORE UPDATE ON content_catalog
             BEGIN SELECT RAISE(FAIL, 'forced metadata sync failure'); END;",
        )
        .unwrap();

        assert!(mark_ai_metadata_pending(&mut conn, &id, &hash).is_err());
        let metadata = get_ai_metadata(&conn, &id).unwrap().unwrap();
        assert_eq!(metadata.status, AiMetadataStatus::Ready);
        assert_eq!(metadata.search_aliases, vec!["ready alias"]);
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            before_revision
        );

        conn.execute("DROP TRIGGER fail_metadata_catalog_update", [])
            .unwrap();
        mark_ai_metadata_pending(&mut conn, &id, &hash).unwrap();
        let pending_revision = crate::content::catalog::current_revision(&conn).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_error_catalog_update
             BEFORE UPDATE ON content_catalog
             BEGIN SELECT RAISE(FAIL, 'forced error sync failure'); END;",
        )
        .unwrap();
        assert!(mark_ai_metadata_error_if_pending(&mut conn, &id, &hash).is_err());
        assert_eq!(
            get_ai_metadata(&conn, &id).unwrap().unwrap().status,
            AiMetadataStatus::Pending
        );
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            pending_revision
        );
    }

    #[test]
    fn organized_vault_entries_always_get_distinct_saved_top_positions() {
        let mut conn = open_unified_db();
        let first = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Note,
                title: "First".into(),
                fields: Vec::new(),
                notes: None,
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        let second =
            create_from_capture(&mut conn, &make_capture_draft("Second"), "saved-top").unwrap();
        let first_position: f64 = conn
            .query_row(
                "SELECT saved_position FROM content_catalog WHERE source_id=?1",
                params![first.entry.id],
                |row| row.get(0),
            )
            .unwrap();
        let (retention, cleanup_at, second_position): (String, Option<String>, f64) = conn
            .query_row(
                "SELECT retention_state, cleanup_at, saved_position
                 FROM content_catalog WHERE source_id=?1",
                params![second.entry.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(retention, "saved");
        assert_eq!(cleanup_at, None);
        assert!(second_position < first_position);
        assert_ne!(second_position, first_position);
    }

    #[test]
    fn default_sensitive_keys_stay_private_even_if_a_legacy_flag_is_false() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "Legacy".into(),
                fields: vec![
                    FieldInput {
                        key: "username".into(),
                        value: "alice".into(),
                        is_sensitive: false,
                    },
                    FieldInput {
                        key: "password".into(),
                        value: "LegacySecretValue".into(),
                        is_sensitive: true,
                    },
                ],
                notes: None,
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        conn.execute(
            "UPDATE vault_fields SET is_sensitive=0
             WHERE entry_id=?1 AND key='password'",
            params![detail.entry.id],
        )
        .unwrap();

        set_manual_tags(&mut conn, &detail.entry.id, &["refreshed".into()]).unwrap();

        let unified: (String, String, String, String) = conn
            .query_row(
                "SELECT title, body, tags, aliases FROM content_fts WHERE unified_id=?1",
                params![format!("vault:{}", detail.entry.id)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        let legacy: (String, String, String) = conn
            .query_row(
                "SELECT title, notes, searchable FROM vault_fts WHERE entry_id=?1",
                params![detail.entry.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        for safe_text in [
            unified.0, unified.1, unified.2, unified.3, legacy.0, legacy.1, legacy.2,
        ] {
            assert!(!safe_text.contains("LegacySecretValue"));
        }
        let preview = list_entries(&conn, None).unwrap()[0]
            .preview
            .clone()
            .unwrap_or_default();
        assert!(!preview.contains("LegacySecretValue"));
    }

    #[test]
    fn semantic_noops_keep_revision_and_changes_empty() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Note,
                title: "Noop".into(),
                fields: Vec::new(),
                notes: Some("body".into()),
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        let id = detail.entry.id;

        set_manual_tags_with_revision(&mut conn, &id, &["Prod".into()]).unwrap();
        let revision = crate::content::catalog::current_revision(&conn).unwrap();
        let manual_noop =
            set_manual_tags_with_revision(&mut conn, &id, &[" prod ".into(), "PROD".into()])
                .unwrap();
        assert_eq!(manual_noop.revision, revision);
        assert!(manual_noop.changes.is_empty());

        replace_ai_tags_with_revision(&mut conn, &id, &["Ops".into()]).unwrap();
        let revision = crate::content::catalog::current_revision(&conn).unwrap();
        let ai_noop =
            replace_ai_tags_with_revision(&mut conn, &id, &[" ops ".into(), "OPS".into()]).unwrap();
        assert_eq!(ai_noop.revision, revision);
        assert!(ai_noop.changes.is_empty());
        let remove_noop = remove_ai_tag_with_revision(&mut conn, &id, "missing").unwrap();
        assert_eq!(remove_noop.revision, revision);
        assert!(remove_noop.changes.is_empty());

        let hash = ai_content_hash_for_entry(&conn, &id).unwrap();
        let metadata = VaultAiMetadata {
            entry_id: id.clone(),
            summary: Some("summary".into()),
            search_aliases: vec!["alias".into()],
            content_hash: hash.clone(),
            provider_id: Some("provider".into()),
            model: Some("model".into()),
            generated_at: Some("2026-07-18T00:00:00Z".into()),
            status: AiMetadataStatus::Ready,
        };
        set_ai_metadata_with_revision(&mut conn, &metadata).unwrap();
        let revision = crate::content::catalog::current_revision(&conn).unwrap();
        let metadata_noop = set_ai_metadata_with_revision(&mut conn, &metadata).unwrap();
        assert_eq!(metadata_noop.revision, revision);
        assert!(metadata_noop.changes.is_empty());

        let pending = mark_ai_metadata_pending_with_revision(&mut conn, &id, &hash).unwrap();
        assert!(!pending.changes.is_empty());
        let revision = pending.revision;
        let pending_noop = mark_ai_metadata_pending_with_revision(&mut conn, &id, &hash).unwrap();
        assert_eq!(pending_noop.revision, revision);
        assert!(pending_noop.changes.is_empty());

        let error = mark_ai_metadata_error_if_pending(&mut conn, &id, &hash)
            .unwrap()
            .unwrap();
        assert!(!error.changes.is_empty());
        let revision = error.revision;
        assert!(mark_ai_metadata_error_if_pending(&mut conn, &id, &hash)
            .unwrap()
            .is_none());
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            revision
        );

        let mut dispatch_count = 0;
        crate::dispatch_content_changed(&metadata_noop, |_event_name, _payload| {
            dispatch_count += 1;
            Ok::<(), ()>(())
        });
        assert_eq!(dispatch_count, 0);
    }

    #[test]
    fn refresh_ai_metadata_uses_the_current_payload_hash_in_one_transaction() {
        let mut conn = open_unified_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Note,
                title: "Current".into(),
                fields: Vec::new(),
                notes: Some("current body".into()),
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        let id = detail.entry.id;
        conn.execute(
            "UPDATE vault_ai_metadata SET content_hash='stale-hash', status='ready'
             WHERE entry_id=?1",
            params![id],
        )
        .unwrap();

        let mutation = refresh_ai_metadata_with_revision(&mut conn, &id).unwrap();

        let current_detail = get_entry_detail(&conn, &id).unwrap();
        let expected_hash =
            compute_entry_content_hash(&current_detail.entry, &current_detail.fields);
        let metadata = current_detail.ai_metadata.unwrap();
        assert_eq!(metadata.status, AiMetadataStatus::Pending);
        assert_eq!(metadata.content_hash, expected_hash);
        assert_eq!(mutation.revision, 2);
        assert_eq!(mutation.changes[0].operation, ContentOperation::Updated);
    }

    #[test]
    fn ensure_vault_schema_creates_all_tables() {
        let conn = open_test_db();
        for table in ["vault_entries", "vault_fields", "vault_tags", "vault_fts"] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
                    params![table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing table {table}");
        }
    }

    #[test]
    fn ensure_vault_schema_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_vault_schema(&mut conn).unwrap();
        ensure_vault_schema(&mut conn).unwrap();
    }

    #[test]
    fn partial_unified_schemas_never_fall_back_to_vault_only_writes() {
        for missing in ["content_state", "content_catalog", "content_fts"] {
            let mut conn = open_unified_db();
            conn.execute(&format!("DROP TABLE {missing}"), []).unwrap();
            let before_entries: i64 = conn
                .query_row("SELECT COUNT(*) FROM vault_entries", [], |row| row.get(0))
                .unwrap();
            let before_revision = if missing == "content_state" {
                None
            } else {
                Some(crate::content::catalog::current_revision(&conn).unwrap())
            };

            let result = create_entry(
                &mut conn,
                &VaultEntryInput {
                    kind: EntryKind::Note,
                    title: "must not commit".into(),
                    fields: Vec::new(),
                    notes: None,
                    manual_tags: Vec::new(),
                },
            );

            assert!(
                matches!(result, Err(StorageError::Migration(_))),
                "missing {missing} must be rejected before writing"
            );
            let direct_result = create_entry_with_revision(
                &mut conn,
                &VaultEntryInput {
                    kind: EntryKind::Note,
                    title: "direct mutation must not commit".into(),
                    fields: Vec::new(),
                    notes: None,
                    manual_tags: Vec::new(),
                },
            );
            assert!(
                matches!(direct_result, Err(StorageError::Migration(_))),
                "direct mutation must use the same schema guard for missing {missing}"
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM vault_entries", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
                before_entries
            );
            if let Some(before_revision) = before_revision {
                assert_eq!(
                    crate::content::catalog::current_revision(&conn).unwrap(),
                    before_revision
                );
            }
        }
    }

    #[test]
    fn fts5_unicode61_tokenizer_supports_cjk() {
        let conn = open_test_db();
        conn.execute(
            "INSERT INTO vault_fts(entry_id, title, notes, searchable) VALUES (?1, ?2, '', ?3)",
            params!["v1", "生产数据库", "生产 数据库"],
        )
        .unwrap();
        let hit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vault_fts WHERE vault_fts MATCH '生产'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(hit >= 1);
    }

    #[test]
    fn create_entry_persists_entry_and_fields() {
        let mut conn = open_test_db();
        let input = VaultEntryInput {
            kind: EntryKind::Credential,
            title: "Prod DB".into(),
            fields: vec![
                FieldInput {
                    key: "user".into(),
                    value: "admin".into(),
                    is_sensitive: false,
                },
                FieldInput {
                    key: "password".into(),
                    value: "s3cr3t".into(),
                    is_sensitive: false,
                },
            ],
            notes: Some("prod".into()),
            manual_tags: Vec::new(),
        };
        let detail = create_entry(&mut conn, &input).unwrap();
        assert_eq!(detail.entry.title, "Prod DB");

        let fields = list_fields(&conn, &detail.entry.id).unwrap();
        assert_eq!(fields.len(), 2);
        // 'password' 字段应被自动标记为 sensitive
        let pwd = fields.iter().find(|f| f.key == "password").unwrap();
        assert!(pwd.is_sensitive, "password should default to sensitive");
        let user = fields.iter().find(|f| f.key == "user").unwrap();
        assert!(!user.is_sensitive);
    }

    #[test]
    fn create_entry_rejects_unknown_kind() {
        let conn = open_test_db();
        // 直接 SQL 注入非法 kind 来测试 CHECK 约束
        let result = conn.execute(
            "INSERT INTO vault_entries(id, kind, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params!["v1", "bogus", "x", "t", "t"],
        );
        assert!(result.is_err());
    }

    /// 旧测试辅助：返回 VaultEntry（不含 tags/metadata）
    fn make_entry(conn: &mut Connection, title: &str) -> VaultEntry {
        create_entry(
            conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: title.into(),
                fields: vec![FieldInput {
                    key: "password".into(),
                    value: "x".into(),
                    is_sensitive: false,
                }],
                notes: None,
                manual_tags: Vec::new(),
            },
        )
        .unwrap()
        .entry
    }

    #[test]
    fn update_entry_replaces_fields_and_bumps_updated_at() {
        let mut conn = open_test_db();
        let e = make_entry(&mut conn, "Original");
        let original_updated = e.updated_at.clone();

        std::thread::sleep(std::time::Duration::from_millis(10));

        let detail = update_entry(
            &mut conn,
            &e.id,
            &VaultEntryInput {
                kind: EntryKind::Bookmark,
                title: "Renamed".into(),
                fields: vec![FieldInput {
                    key: "url".into(),
                    value: "https://x.com".into(),
                    is_sensitive: false,
                }],
                notes: Some("n".into()),
                manual_tags: Vec::new(),
            },
        )
        .unwrap();

        assert_eq!(detail.entry.title, "Renamed");
        assert_eq!(detail.entry.kind, EntryKind::Bookmark);
        assert_ne!(detail.entry.updated_at, original_updated);
        assert_eq!(detail.fields.len(), 1);
        assert_eq!(detail.fields[0].key, "url");
    }

    #[test]
    fn delete_entry_cascades_fields_and_tags() {
        let mut conn = open_test_db();
        let e = make_entry(&mut conn, "To Delete");
        set_manual_tags(&mut conn, &e.id, &["t1".into(), "t2".into()]).unwrap();
        delete_entry(&mut conn, &e.id).unwrap();

        assert!(get_entry_by_id(&conn, &e.id).unwrap().is_none());
        assert!(list_fields(&conn, &e.id).unwrap().is_empty());
        assert!(list_tags(&conn, &e.id).unwrap().is_empty());
    }

    #[test]
    fn list_entries_filters_by_kind_and_orders_by_updated_desc() {
        let mut conn = open_test_db();
        let _a = make_entry(&mut conn, "A");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _b = make_entry(&mut conn, "B");
        let entries = list_entries(&conn, None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].entry.title, "B"); // newer first
    }

    #[test]
    fn list_entries_with_kind_filter() {
        let mut conn = open_test_db();
        create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Bookmark,
                title: "BM".into(),
                fields: vec![],
                notes: None,
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "Cred".into(),
                fields: vec![],
                notes: None,
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        let bm = list_entries(&conn, Some(EntryKind::Bookmark)).unwrap();
        assert_eq!(bm.len(), 1);
        assert_eq!(bm[0].entry.title, "BM");
    }

    #[test]
    fn set_manual_tags_replaces_existing() {
        let mut conn = open_test_db();
        let e = make_entry(&mut conn, "T");
        set_manual_tags(&mut conn, &e.id, &["a".into()]).unwrap();
        set_manual_tags(&mut conn, &e.id, &["b".into(), "c".into()]).unwrap();
        let mut tags: Vec<String> = list_tags(&conn, &e.id)
            .unwrap()
            .into_iter()
            .map(|t| t.tag)
            .collect();
        tags.sort();
        assert_eq!(tags, vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn fts5_indexes_title_username_and_tags_not_password() {
        let mut conn = open_test_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "Production Database".into(),
                fields: vec![
                    FieldInput {
                        key: "user".into(),
                        value: "admin".into(),
                        is_sensitive: false,
                    },
                    FieldInput {
                        key: "password".into(),
                        value: "supersecretvalue".into(),
                        is_sensitive: false,
                    },
                ],
                notes: Some("mysql prod".into()),
                manual_tags: Vec::new(),
            },
        )
        .unwrap();
        let entry_id = detail.entry.id.clone();
        set_manual_tags(&mut conn, &entry_id, &["mysql".into(), "prod".into()]).unwrap();

        // 搜 title
        let hits = fts5_search(&conn, "production", 10).unwrap();
        assert!(hits.iter().any(|(id, _)| id == &entry_id));

        // 搜 username
        let hits = fts5_search(&conn, "admin", 10).unwrap();
        assert!(hits.iter().any(|(id, _)| id == &entry_id));

        // 搜 tag
        let hits = fts5_search(&conn, "mysql", 10).unwrap();
        assert!(hits.iter().any(|(id, _)| id == &entry_id));

        // 不能搜 password
        let hits = fts5_search(&conn, "supersecretvalue", 10).unwrap();
        assert!(!hits.iter().any(|(id, _)| id == &entry_id));
    }

    #[test]
    fn fts5_search_after_delete_returns_nothing() {
        let mut conn = open_test_db();
        let e = make_entry(&mut conn, "DeleteMe");
        assert!(!fts5_search(&conn, "DeleteMe", 10).unwrap().is_empty());
        delete_entry(&mut conn, &e.id).unwrap();
        assert!(fts5_search(&conn, "DeleteMe", 10).unwrap().is_empty());
    }

    // ============================================================
    // Task 4: Atomic repository behaviors
    // ============================================================

    fn input_with_manual_tags(tags: &[&str]) -> VaultEntryInput {
        VaultEntryInput {
            kind: EntryKind::Credential,
            title: "Production DB".into(),
            fields: vec![FieldInput {
                key: "password".into(),
                value: "hunter2".into(),
                is_sensitive: true,
            }],
            notes: None,
            manual_tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        }
    }

    #[test]
    fn replacing_ai_tags_preserves_manual_tags() {
        let mut conn = open_test_db();
        let detail = create_entry(&mut conn, &input_with_manual_tags(&["数据库"])).unwrap();
        replace_ai_tags(
            &mut conn,
            &detail.entry.id,
            &["生产".into(), "数据库".into()],
        )
        .unwrap();
        replace_ai_tags(&mut conn, &detail.entry.id, &["MySQL".into()]).unwrap();

        let tags = list_tags(&conn, &detail.entry.id).unwrap();
        assert!(tags
            .iter()
            .any(|t| t.tag == "数据库" && t.source == TagSource::Manual));
        assert!(tags
            .iter()
            .any(|t| t.tag == "MySQL" && t.source == TagSource::Ai));
        assert!(!tags.iter().any(|t| t.tag == "生产"));
    }

    #[test]
    fn create_entry_saves_manual_tags_and_pending_metadata_atomically() {
        let mut conn = open_test_db();
        let detail =
            create_entry(&mut conn, &input_with_manual_tags(&["数据库", "MySQL"])).unwrap();

        // manual tags 落库
        let tags = list_tags(&conn, &detail.entry.id).unwrap();
        assert!(tags
            .iter()
            .any(|t| t.tag == "数据库" && t.source == TagSource::Manual));
        assert!(tags
            .iter()
            .any(|t| t.tag == "MySQL" && t.source == TagSource::Manual));
        assert!(tags.iter().all(|t| t.source == TagSource::Manual));

        // AI metadata 落库 + pending 状态
        let md = get_ai_metadata(&conn, &detail.entry.id).unwrap();
        let md = md.expect("metadata should exist after create_entry");
        assert_eq!(md.status, AiMetadataStatus::Pending);
        assert!(
            !md.content_hash.is_empty(),
            "content hash should be populated"
        );
        assert!(md.summary.is_none());
        assert!(md.search_aliases.is_empty());
    }

    #[test]
    fn update_entry_removes_stale_ai_tags_but_preserves_manual_tags() {
        let mut conn = open_test_db();
        // 创建带 manual tag 的 entry
        let detail = create_entry(&mut conn, &input_with_manual_tags(&["数据库"])).unwrap();
        let id = detail.entry.id.clone();
        // 注入 AI tags（模拟 AI 已分析完毕）
        replace_ai_tags(&mut conn, &id, &["生产".into(), "MySQL".into()]).unwrap();
        // 读出当前 content hash（必须在 mutable borrow 之前）
        let orig_hash = ai_content_hash_for_entry(&conn, &id).unwrap();
        // 把 metadata 改为 ready，方便后续断言"内容变化时被重置"
        set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: id.clone(),
                summary: Some("prod db".into()),
                search_aliases: vec!["prod".into()],
                content_hash: orig_hash,
                provider_id: Some("openai".into()),
                model: Some("gpt-4".into()),
                generated_at: Some("2026-07-01T00:00:00Z".into()),
                status: AiMetadataStatus::Ready,
            },
        )
        .unwrap();

        // 内容真的改变（title 变化 → hash 变化）
        let mut new_input = input_with_manual_tags(&["数据库"]);
        new_input.title = "Staging DB".into();
        update_entry(&mut conn, &id, &new_input).unwrap();

        let tags = list_tags(&conn, &id).unwrap();
        // manual 保留
        assert!(tags
            .iter()
            .any(|t| t.tag == "数据库" && t.source == TagSource::Manual));
        // ai 全部被清掉
        assert!(!tags.iter().any(|t| t.source == TagSource::Ai));

        // metadata 被重置为 pending
        let md = get_ai_metadata(&conn, &id).unwrap().unwrap();
        assert_eq!(md.status, AiMetadataStatus::Pending);
        assert!(md.summary.is_none());
    }

    #[test]
    fn manual_tag_only_update_preserves_ready_ai_metadata() {
        let mut conn = open_test_db();
        let detail = create_entry(&mut conn, &input_with_manual_tags(&["数据库"])).unwrap();
        let id = detail.entry.id.clone();

        // AI 已分析：注入 ai tags + ready metadata
        replace_ai_tags(&mut conn, &id, &["生产".into()]).unwrap();
        let orig_hash = ai_content_hash_for_entry(&conn, &id).unwrap();
        set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: id.clone(),
                summary: Some("prod".into()),
                search_aliases: vec!["prod-alias".into()],
                content_hash: orig_hash.clone(),
                provider_id: Some("openai".into()),
                model: Some("gpt-4".into()),
                generated_at: Some("2026-07-01T00:00:00Z".into()),
                status: AiMetadataStatus::Ready,
            },
        )
        .unwrap();

        // 仅改 manual_tags（kind/title/notes/fields 不变 → hash 不变）
        let new_input = input_with_manual_tags(&["数据库", "MySQL", "重要"]);
        // 完全保留原 kind/title/fields/notes，仅扩展 manual_tags
        update_entry(&mut conn, &id, &new_input).unwrap();

        // ai tags 必须仍在
        let tags = list_tags(&conn, &id).unwrap();
        assert!(
            tags.iter()
                .any(|t| t.tag == "生产" && t.source == TagSource::Ai),
            "AI tags must be preserved when content hash unchanged"
        );
        // 新 manual tag 落库
        assert!(tags
            .iter()
            .any(|t| t.tag == "MySQL" && t.source == TagSource::Manual));
        assert!(tags
            .iter()
            .any(|t| t.tag == "重要" && t.source == TagSource::Manual));

        // metadata 仍是 ready，summary 仍存在
        let md = get_ai_metadata(&conn, &id).unwrap().unwrap();
        assert_eq!(
            md.status,
            AiMetadataStatus::Ready,
            "ready metadata must be preserved"
        );
        assert_eq!(md.summary.as_deref(), Some("prod"));
        assert_eq!(md.content_hash, orig_hash);
    }

    #[test]
    fn remove_ai_tag_never_removes_same_named_manual_tag() {
        let mut conn = open_test_db();
        // 同时设置同名 manual + ai 标签
        let detail = create_entry(&mut conn, &input_with_manual_tags(&["prod"])).unwrap();
        let id = detail.entry.id.clone();
        replace_ai_tags(&mut conn, &id, &["prod".into()]).unwrap();

        // 删除 AI 版本
        remove_ai_tag(&mut conn, &id, "prod").unwrap();

        let tags = list_tags(&conn, &id).unwrap();
        // manual 必须保留
        assert!(
            tags.iter()
                .any(|t| t.tag == "prod" && t.source == TagSource::Manual),
            "manual tag must survive remove_ai_tag"
        );
        // ai 必须被删除
        assert!(
            !tags.iter().any(|t| t.source == TagSource::Ai),
            "ai tag should be removed"
        );
    }

    fn make_capture_draft(title: &str) -> CaptureDraft {
        CaptureDraft {
            kind: EntryKind::Credential,
            title: title.into(),
            notes: Some("from capture".into()),
            fields: vec![CaptureField {
                draft_id: "d1".into(),
                key: "password".into(),
                value: "topsecret".into(),
                is_sensitive: true,
            }],
            manual_tags: vec!["手输".into()],
            ai_tags: vec!["AI".into()],
            ai_summary: Some("summary".into()),
            search_aliases: vec!["alias1".into()],
            ai_provenance: Some(AiProvenance {
                provider_id: "openai".into(),
                model: "gpt-4".into(),
                generated_at: "2026-07-01T00:00:00Z".into(),
            }),
            warnings: vec![],
        }
    }

    #[test]
    fn capture_request_id_returns_existing_entry_on_retry() {
        let mut conn = open_test_db();
        let draft = make_capture_draft("Capture One");
        let d1 = create_from_capture(&mut conn, &draft, "req-001").unwrap();
        // 第二次用相同 request_id：应返回同一条 entry，不应新建
        let d2 = create_from_capture(&mut conn, &draft, "req-001").unwrap();
        assert_eq!(d1.entry.id, d2.entry.id);

        // 只有一行 capture request 记录
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vault_capture_requests WHERE request_id='req-001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        // 只有一条 entry
        let n_entries: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vault_entries WHERE title='Capture One'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n_entries, 1);
    }

    #[test]
    fn concurrent_capture_requests_across_connections_are_transparently_idempotent() {
        let db_path = std::env::temp_dir().join(format!(
            "scratchpad-capture-race-{}-{}.sqlite",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let mut setup = Connection::open(&db_path).unwrap();
        setup.pragma_update(None, "foreign_keys", "ON").unwrap();
        setup.pragma_update(None, "journal_mode", "WAL").unwrap();
        crate::scratchpad::storage::ensure_dock_schema(&mut setup).unwrap();
        ensure_vault_schema(&mut setup).unwrap();
        crate::content::migrations::ensure_content_schema(&mut setup, 7).unwrap();
        drop(setup);

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let draft = make_capture_draft("Concurrent Capture");
        let spawn_call = |barrier: std::sync::Arc<std::sync::Barrier>| {
            let path = db_path.clone();
            let draft = draft.clone();
            std::thread::spawn(move || {
                let mut conn = Connection::open(path).unwrap();
                conn.pragma_update(None, "foreign_keys", "ON").unwrap();
                conn.busy_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                barrier.wait();
                create_from_capture_with_revision(&mut conn, &draft, "same-request")
            })
        };
        let first = spawn_call(barrier.clone());
        let second = spawn_call(barrier);
        let first = first.join().unwrap().unwrap();
        let second = second.join().unwrap().unwrap();

        assert_eq!(first.value.entry.id, second.value.entry.id);
        assert_eq!(first.revision, 1);
        assert_eq!(second.revision, 1);
        let change_counts = [first.changes.len(), second.changes.len()];
        assert!(change_counts.contains(&1));
        assert!(change_counts.contains(&0));

        let verify = Connection::open(&db_path).unwrap();
        for (table, predicate) in [
            ("vault_entries", "title='Concurrent Capture'"),
            ("vault_capture_requests", "request_id='same-request'"),
            (
                "content_catalog",
                "source_id IN (SELECT id FROM vault_entries)",
            ),
            ("vault_fts", "entry_id IN (SELECT id FROM vault_entries)"),
            (
                "content_fts",
                "unified_id IN (SELECT 'vault:' || id FROM vault_entries)",
            ),
        ] {
            let count: i64 = verify
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE {predicate}"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "unexpected rows in {table}");
        }
        assert_eq!(
            crate::content::catalog::current_revision(&verify).unwrap(),
            1
        );
        drop(verify);
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn failed_capture_transaction_leaves_no_request_or_partial_entry() {
        let mut conn = open_test_db();
        // 预占一个 id 使后续 INSERT 失败：先插入一条 entry 用固定 id
        conn.execute(
            "INSERT INTO vault_entries(id, kind, title, created_at, updated_at)
             VALUES ('fixed-id', 'credential', 'blocker', 't', 't')",
            [],
        )
        .unwrap();
        // 构造 draft，让其必然失败：我们让 fields 里包含会违反 UNIQUE 的 key——
        // 但更可靠的办法是制造 FK/约束失败。这里用事务回滚验证：
        // 模拟：capture 过程中段失败（用同一 request_id + 人为破坏）。
        // 直接验证事务原子性：capture 中途任何错误都不应留下半成品。
        // 我们通过让 capture draft 的 field key 重复（UNIQUE(entry_id,key)）来触发失败。
        let mut draft = make_capture_draft("Will Fail");
        draft.fields.push(CaptureField {
            draft_id: "d1".into(),
            key: "password".into(), // 与第一个 field 同 key → UNIQUE 冲突
            value: "dup".into(),
            is_sensitive: true,
        });
        let result = create_from_capture(&mut conn, &draft, "req-fail");
        assert!(
            result.is_err(),
            "capture with duplicate field key should fail"
        );

        // 失败后：没有 capture_requests 记录
        let n_req: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vault_capture_requests WHERE request_id='req-fail'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            n_req, 0,
            "no request row should remain after failed capture"
        );
        // 失败后：没有 title='Will Fail' 的 entry
        let n_entries: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vault_entries WHERE title='Will Fail'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            n_entries, 0,
            "no partial entry should remain after failed capture"
        );
        // 失败后：对应的 metadata 也不应存在
        let n_md: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vault_ai_metadata WHERE summary='summary'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            n_md, 0,
            "no metadata row should remain after failed capture"
        );
    }

    #[test]
    fn capture_with_provenance_and_content_saved_as_ready() {
        // spec: capture 若已有合法 AI metadata 则直接保存 ready，不再调 LLM。
        let mut conn = open_test_db();
        let draft = make_capture_draft("Ready Capture");
        // make_capture_draft 默认携带 provenance + summary + alias + ai-tag
        // → 必须保存为 ready。
        let saved = create_from_capture(&mut conn, &draft, "req-ready-1").unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM vault_ai_metadata WHERE entry_id=?1",
                params![saved.entry.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            status, "ready",
            "draft with provenance + content must be ready"
        );
        // provider_id 必须落盘
        let provider: Option<String> = conn
            .query_row(
                "SELECT provider_id FROM vault_ai_metadata WHERE entry_id=?1",
                params![saved.entry.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(provider.as_deref(), Some("openai"));
    }

    #[test]
    fn capture_without_provenance_saved_as_pending() {
        // 没有 provenance → 必须 pending，等待 backfill worker 拾起。
        let mut conn = open_test_db();
        let mut draft = make_capture_draft("Pending Capture");
        draft.ai_provenance = None;
        // 即便有 summary 也不行：缺 provenance 必须走 pending
        let saved = create_from_capture(&mut conn, &draft, "req-pending-1").unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM vault_ai_metadata WHERE entry_id=?1",
                params![saved.entry.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            status, "pending",
            "draft without provenance must be pending"
        );
        // provider_id 必须为 NULL
        let provider: Option<String> = conn
            .query_row(
                "SELECT provider_id FROM vault_ai_metadata WHERE entry_id=?1",
                params![saved.entry.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(provider.is_none());
    }

    #[test]
    fn capture_with_provenance_but_empty_content_saved_as_pending() {
        // 有 provenance 但没有 summary/aliases/ai-tags → 仍需 backfill 补全
        let mut conn = open_test_db();
        let mut draft = make_capture_draft("Half Capture");
        draft.ai_summary = None;
        draft.search_aliases.clear();
        draft.ai_tags.clear();
        let saved = create_from_capture(&mut conn, &draft, "req-half-1").unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM vault_ai_metadata WHERE entry_id=?1",
                params![saved.entry.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            status, "pending",
            "draft with provenance but no AI content must be pending"
        );
    }

    #[test]
    fn content_hash_ignores_sensitive_value_rotation() {
        let mut input_v1 = VaultEntryInput {
            kind: EntryKind::Credential,
            title: "Prod".into(),
            fields: vec![
                FieldInput {
                    key: "user".into(),
                    value: "admin".into(),
                    is_sensitive: false,
                },
                FieldInput {
                    key: "password".into(),
                    value: "hunter2".into(),
                    is_sensitive: false,
                },
            ],
            notes: Some("notes".into()),
            manual_tags: vec![],
        };
        let h1 = ai_content_hash(&input_v1);

        // 轮换密码：hash 应保持不变
        input_v1.fields[1].value = "totally-different-pw".into();
        let h2 = ai_content_hash(&input_v1);
        assert_eq!(
            h1, h2,
            "rotating sensitive value must not change content hash"
        );

        // 改非敏感字段 value → hash 必须变化
        input_v1.fields[0].value = "root".into();
        let h3 = ai_content_hash(&input_v1);
        assert_ne!(h1, h3, "changing non-sensitive value must change hash");
    }

    #[test]
    fn entry_summary_preview_never_contains_sensitive_values() {
        let mut conn = open_test_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "Preview Test".into(),
                fields: vec![
                    FieldInput {
                        key: "user".into(),
                        value: "admin".into(),
                        is_sensitive: false,
                    },
                    FieldInput {
                        key: "password".into(),
                        value: "DO_NOT_LEAK".into(),
                        is_sensitive: false, // 即使前端忘传，create_entry 也会按 default 标敏感
                    },
                ],
                notes: None,
                manual_tags: vec![],
            },
        )
        .unwrap();

        let summaries = list_entries(&conn, None).unwrap();
        let s = summaries
            .iter()
            .find(|s| s.entry.id == detail.entry.id)
            .unwrap();
        let preview = s.preview.clone().unwrap_or_default();
        assert!(
            preview.contains("admin"),
            "preview should include non-sensitive value"
        );
        assert!(
            !preview.contains("DO_NOT_LEAK"),
            "preview must not contain sensitive value"
        );
    }

    #[test]
    fn search_index_never_contains_sensitive_values() {
        let mut conn = open_test_db();
        let detail = create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "Index Test".into(),
                fields: vec![
                    FieldInput {
                        key: "user".into(),
                        value: "admin".into(),
                        is_sensitive: false,
                    },
                    FieldInput {
                        key: "password".into(),
                        value: "INDEX_SECRET_VALUE".into(),
                        is_sensitive: false,
                    },
                ],
                notes: None,
                manual_tags: vec![],
            },
        )
        .unwrap();
        let id = detail.entry.id.clone();

        // 通过 FTS 无法召回敏感值
        let hits = fts5_search(&conn, "INDEX_SECRET_VALUE", 10).unwrap();
        assert!(
            !hits.iter().any(|(h_id, _)| h_id == &id),
            "FTS must never index sensitive values"
        );

        // 但能召回非敏感值
        let hits = fts5_search(&conn, "admin", 10).unwrap();
        assert!(hits.iter().any(|(h_id, _)| h_id == &id));
    }
}
