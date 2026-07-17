use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use rusqlite::{params, Connection, Row};
use sha2::{Digest, Sha256};

use crate::storage::error::{StorageError, StorageResult};
use crate::vault::models::{
    AiMetadataStatus, BackfillStatus, CaptureDraft, EntryKind, TagSource, VaultAiMetadata,
    VaultEntry, VaultEntryDetail, VaultEntryInput, VaultEntrySummary, VaultField, VaultTag,
    is_default_sensitive_key,
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

pub fn create_entry(
    conn: &mut Connection,
    input: &VaultEntryInput,
) -> StorageResult<VaultEntryDetail> {
    let tx = conn.transaction()?;
    let id = next_entry_id();
    let now = now_rfc3339();
    tx.execute(
        "INSERT INTO vault_entries(id, kind, title, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, input.kind.as_str(), input.title, input.notes, now, now],
    )?;

    for (i, f) in input.fields.iter().enumerate() {
        let sensitive = f.is_sensitive || is_default_sensitive_key(&f.key);
        let fid = next_field_id();
        tx.execute(
            "INSERT INTO vault_fields(id, entry_id, key, value, is_sensitive, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![fid, id, f.key, f.value, sensitive as i32, i as i64],
        )?;
    }

    // 写入手动标签（manual source）
    write_manual_tags(&tx, &id, &input.manual_tags)?;

    // 写入 pending AI metadata + 触发 FTS 更新（同事务）
    let new_hash = ai_content_hash(input);
    upsert_ai_metadata_pending(&tx, &id, &new_hash)?;
    fts5_upsert(&tx, &id)?;

    tx.commit()?;
    get_entry_detail(conn, &id)
}

pub fn list_fields(conn: &Connection, entry_id: &str) -> StorageResult<Vec<VaultField>> {
    let mut stmt = conn.prepare(
        "SELECT id, entry_id, key, value, is_sensitive, sort_order
         FROM vault_fields WHERE entry_id = ?1 ORDER BY sort_order",
    )?;
    let rows = stmt.query_map(params![entry_id], row_to_field)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(StorageError::from)
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
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad kind")),
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
    let tx = conn.transaction()?;
    let now = now_rfc3339();

    // 读取旧 hash（如有）以判断内容是否变化
    let old_hash: Option<String> = tx
        .query_row(
            "SELECT content_hash FROM vault_ai_metadata WHERE entry_id=?1",
            params![id],
            |r| r.get::<_, String>(0),
        )
        .ok();

    let affected = tx.execute(
        "UPDATE vault_entries SET kind=?1, title=?2, notes=?3, updated_at=?4 WHERE id=?5",
        params![input.kind.as_str(), input.title, input.notes, now, id],
    )?;
    if affected == 0 {
        return Err(StorageError::Other(format!("entry not found: {id}")));
    }
    tx.execute("DELETE FROM vault_fields WHERE entry_id=?1", params![id])?;
    for (i, f) in input.fields.iter().enumerate() {
        let sensitive = f.is_sensitive || is_default_sensitive_key(&f.key);
        tx.execute(
            "INSERT INTO vault_fields(id, entry_id, key, value, is_sensitive, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![next_field_id(), id, f.key, f.value, sensitive as i32, i as i64],
        )?;
    }

    // 用 manual_tags 覆盖手动标签（不影响 AI 标签）
    write_manual_tags(&tx, id, &input.manual_tags)?;

    // 比较 content hash：仅当内容真的变化时清除 AI tags 并把 metadata 重置为 pending。
    let new_hash = ai_content_hash(input);
    let content_changed = old_hash.as_deref() != Some(new_hash.as_str());
    if content_changed {
        // 清掉旧的 AI tags（manual 不动），并把 metadata 重置为 pending
        tx.execute(
            "DELETE FROM vault_tags WHERE entry_id=?1 AND source='ai'",
            params![id],
        )?;
        upsert_ai_metadata_pending(&tx, id, &new_hash)?;
    }

    // FTS 始终刷新（标题/notes/字段值/标签都可能改变）
    fts5_upsert(&tx, id)?;

    tx.commit()?;
    get_entry_detail(conn, id)
}

pub fn delete_entry(conn: &mut Connection, id: &str) -> StorageResult<()> {
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
            let non_sensitive: Vec<&VaultField> =
                fields.iter().filter(|f| !f.is_sensitive).take(2).collect();
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
                .find(|f| f.key.eq_ignore_ascii_case("url") && !f.is_sensitive)
                .map(|f| f.value.trim())
                .filter(|s| !s.is_empty());
            url.and_then(&mut trim_to)
                .or_else(|| entry.notes.as_deref().and_then(&mut trim_to))
        }
        EntryKind::Note => entry.notes.as_deref().and_then(&mut trim_to),
    };
    candidate
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
    let tx = conn.transaction()?;
    write_manual_tags(&tx, entry_id, tags)?;
    fts5_upsert(&tx, entry_id)?;
    tx.commit()?;
    Ok(())
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

/// 用新的 AI 标签集合替换该 entry 的所有 source='ai' 行（manual 不动）。
pub fn replace_ai_tags(
    conn: &mut Connection,
    entry_id: &str,
    tags: &[String],
) -> StorageResult<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM vault_tags WHERE entry_id=?1 AND source='ai'",
        params![entry_id],
    )?;
    for t in tags {
        if let Some(norm) = crate::vault::migrations::normalize_tag(t) {
            let display = t.trim().to_string();
            tx.execute(
                "INSERT OR IGNORE INTO vault_tags(entry_id, tag, normalized_tag, source)
                 VALUES (?1, ?2, ?3, 'ai')",
                params![entry_id, display, norm],
            )?;
        }
    }
    fts5_upsert(&tx, entry_id)?;
    tx.commit()?;
    Ok(())
}

/// 删除某个 normalized_tag 对应的 AI 行；同名 manual 行永远保留。
pub fn remove_ai_tag(
    conn: &mut Connection,
    entry_id: &str,
    normalized_tag: &str,
) -> StorageResult<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM vault_tags
         WHERE entry_id=?1 AND source='ai' AND normalized_tag=?2",
        params![entry_id, normalized_tag],
    )?;
    fts5_upsert(&tx, entry_id)?;
    tx.commit()?;
    Ok(())
}

/// 写入完整的 ready AI metadata（status='ready'）。同事务刷新 FTS 以反映 search_aliases。
pub fn set_ai_metadata(
    conn: &mut Connection,
    metadata: &VaultAiMetadata,
) -> StorageResult<()> {
    let tx = conn.transaction()?;
    let aliases_json = serde_json::to_string(&metadata.search_aliases)
        .map_err(|e| StorageError::Other(e.to_string()))?;
    tx.execute(
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
    // 状态从 pending→ready 或 ready→pending 时刷新 FTS
    fts5_upsert(&tx, &metadata.entry_id)?;
    tx.commit()?;
    Ok(())
}

/// 把 metadata 置为 pending 状态（保留 entry_id，写入新 content_hash）。
/// 若该 entry 尚无 metadata 行，插入一条 pending 行。
pub fn mark_ai_metadata_pending(
    conn: &mut Connection,
    entry_id: &str,
    content_hash: &str,
) -> StorageResult<()> {
    let tx = conn.transaction()?;
    upsert_ai_metadata_pending(&tx, entry_id, content_hash)?;
    fts5_upsert(&tx, entry_id)?;
    tx.commit()?;
    Ok(())
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
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(StorageError::from)
}

/// 统计当前各 AI metadata 状态的条目数。
/// `processing` 字段始终为 0（worker 没有显式 processing 状态，pending 即"待处理"）。
pub fn backfill_status(conn: &Connection) -> StorageResult<BackfillStatus> {
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vault_ai_metadata",
        [],
        |r| r.get(0),
    )?;
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
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(StorageError::from)
}

/// 读取一条条目的 AI 元数据；无记录时返回 None。
pub fn get_ai_metadata(conn: &Connection, entry_id: &str) -> StorageResult<Option<VaultAiMetadata>> {
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
    let aliases: Vec<String> =
        serde_json::from_str(&aliases_json).unwrap_or_default();
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
        "SELECT value FROM vault_fields WHERE entry_id=?1 AND is_sensitive=0",
    )?;
    let values = stmt.query_map(params![entry_id], |r| r.get::<_, String>(0))?;
    for v in values {
        parts.push(v?);
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
    conn.execute(
        "DELETE FROM vault_fts WHERE entry_id=?1",
        params![entry_id],
    )?;
    conn.execute(
        "INSERT INTO vault_fts(entry_id, title, notes, searchable) VALUES (?1, ?2, ?3, ?4)",
        params![entry_id, entry.title, entry.notes.unwrap_or_default(), searchable],
    )?;
    Ok(())
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
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(StorageError::from)
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
pub fn compute_entry_content_hash(
    entry: &VaultEntry,
    fields: &[VaultField],
) -> String {
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
    // 幂等检查：request_id 已存在则返回原 entry
    let existing_entry_id: Option<String> = conn
        .query_row(
            "SELECT entry_id FROM vault_capture_requests WHERE request_id=?1",
            params![request_id],
            |r| r.get::<_, String>(0),
        )
        .ok();
    if let Some(id) = existing_entry_id {
        return get_entry_detail(conn, &id);
    }

    let tx = conn.transaction()?;
    let id = next_entry_id();
    let now = now_rfc3339();

    tx.execute(
        "INSERT INTO vault_entries(id, kind, title, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, draft.kind.as_str(), draft.title, draft.notes, now, now],
    )?;

    for (i, f) in draft.fields.iter().enumerate() {
        let sensitive = f.is_sensitive || is_default_sensitive_key(&f.key);
        let fid = next_field_id();
        tx.execute(
            "INSERT INTO vault_fields(id, entry_id, key, value, is_sensitive, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![fid, id, f.key, f.value, sensitive as i32, i as i64],
        )?;
    }

    // 写入手动标签
    write_manual_tags(&tx, &id, &draft.manual_tags)?;

    // 写入 AI 标签（source='ai'）
    for t in &draft.ai_tags {
        if let Some(norm) = crate::vault::migrations::normalize_tag(t) {
            let display = t.trim().to_string();
            tx.execute(
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
        Some(p) => (Some(p.provider_id.clone()), Some(p.model.clone()), Some(p.generated_at.clone())),
        None => (None, None, None),
    };
    let has_ai_content = draft.ai_summary.as_ref().is_some_and(|s| !s.trim().is_empty())
        || !draft.search_aliases.is_empty()
        || !draft.ai_tags.is_empty();
    let status_value = if draft.ai_provenance.is_some() && has_ai_content {
        "ready"
    } else {
        "pending"
    };
    tx.execute(
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
    tx.execute(
        "INSERT INTO vault_capture_requests(request_id, entry_id, created_at)
         VALUES (?1, ?2, ?3)",
        params![request_id, id, now],
    )?;

    fts5_upsert(&tx, &id)?;
    tx.commit()?;
    get_entry_detail(conn, &id)
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
                FieldInput { key: "user".into(), value: "admin".into(), is_sensitive: false },
                FieldInput { key: "password".into(), value: "s3cr3t".into(), is_sensitive: false },
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
        create_entry(conn, &VaultEntryInput {
            kind: EntryKind::Credential,
            title: title.into(),
            fields: vec![FieldInput {
                key: "password".into(),
                value: "x".into(),
                is_sensitive: false,
            }],
            notes: None,
            manual_tags: Vec::new(),
        })
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
                    FieldInput { key: "user".into(), value: "admin".into(), is_sensitive: false },
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
        replace_ai_tags(&mut conn, &detail.entry.id, &["生产".into(), "数据库".into()]).unwrap();
        replace_ai_tags(&mut conn, &detail.entry.id, &["MySQL".into()]).unwrap();

        let tags = list_tags(&conn, &detail.entry.id).unwrap();
        assert!(tags.iter().any(|t| t.tag == "数据库" && t.source == TagSource::Manual));
        assert!(tags.iter().any(|t| t.tag == "MySQL" && t.source == TagSource::Ai));
        assert!(!tags.iter().any(|t| t.tag == "生产"));
    }

    #[test]
    fn create_entry_saves_manual_tags_and_pending_metadata_atomically() {
        let mut conn = open_test_db();
        let detail = create_entry(&mut conn, &input_with_manual_tags(&["数据库", "MySQL"])).unwrap();

        // manual tags 落库
        let tags = list_tags(&conn, &detail.entry.id).unwrap();
        assert!(tags.iter().any(|t| t.tag == "数据库" && t.source == TagSource::Manual));
        assert!(tags.iter().any(|t| t.tag == "MySQL" && t.source == TagSource::Manual));
        assert!(tags.iter().all(|t| t.source == TagSource::Manual));

        // AI metadata 落库 + pending 状态
        let md = get_ai_metadata(&conn, &detail.entry.id).unwrap();
        let md = md.expect("metadata should exist after create_entry");
        assert_eq!(md.status, AiMetadataStatus::Pending);
        assert!(!md.content_hash.is_empty(), "content hash should be populated");
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
        assert!(tags.iter().any(|t| t.tag == "数据库" && t.source == TagSource::Manual));
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
        assert!(tags.iter().any(|t| t.tag == "生产" && t.source == TagSource::Ai),
            "AI tags must be preserved when content hash unchanged");
        // 新 manual tag 落库
        assert!(tags.iter().any(|t| t.tag == "MySQL" && t.source == TagSource::Manual));
        assert!(tags.iter().any(|t| t.tag == "重要" && t.source == TagSource::Manual));

        // metadata 仍是 ready，summary 仍存在
        let md = get_ai_metadata(&conn, &id).unwrap().unwrap();
        assert_eq!(md.status, AiMetadataStatus::Ready, "ready metadata must be preserved");
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
        assert!(tags.iter().any(|t| t.tag == "prod" && t.source == TagSource::Manual),
            "manual tag must survive remove_ai_tag");
        // ai 必须被删除
        assert!(!tags.iter().any(|t| t.source == TagSource::Ai),
            "ai tag should be removed");
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
        assert!(result.is_err(), "capture with duplicate field key should fail");

        // 失败后：没有 capture_requests 记录
        let n_req: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vault_capture_requests WHERE request_id='req-fail'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n_req, 0, "no request row should remain after failed capture");
        // 失败后：没有 title='Will Fail' 的 entry
        let n_entries: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vault_entries WHERE title='Will Fail'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n_entries, 0, "no partial entry should remain after failed capture");
        // 失败后：对应的 metadata 也不应存在
        let n_md: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vault_ai_metadata WHERE summary='summary'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n_md, 0, "no metadata row should remain after failed capture");
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
        assert_eq!(status, "ready", "draft with provenance + content must be ready");
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
        assert_eq!(status, "pending", "draft without provenance must be pending");
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
                FieldInput { key: "user".into(), value: "admin".into(), is_sensitive: false },
                FieldInput { key: "password".into(), value: "hunter2".into(), is_sensitive: false },
            ],
            notes: Some("notes".into()),
            manual_tags: vec![],
        };
        let h1 = ai_content_hash(&input_v1);

        // 轮换密码：hash 应保持不变
        input_v1.fields[1].value = "totally-different-pw".into();
        let h2 = ai_content_hash(&input_v1);
        assert_eq!(h1, h2, "rotating sensitive value must not change content hash");

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
                    FieldInput { key: "user".into(), value: "admin".into(), is_sensitive: false },
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
        let s = summaries.iter().find(|s| s.entry.id == detail.entry.id).unwrap();
        let preview = s.preview.clone().unwrap_or_default();
        assert!(preview.contains("admin"), "preview should include non-sensitive value");
        assert!(!preview.contains("DO_NOT_LEAK"), "preview must not contain sensitive value");
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
                    FieldInput { key: "user".into(), value: "admin".into(), is_sensitive: false },
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
        assert!(!hits.iter().any(|(h_id, _)| h_id == &id),
            "FTS must never index sensitive values");

        // 但能召回非敏感值
        let hits = fts5_search(&conn, "admin", 10).unwrap();
        assert!(hits.iter().any(|(h_id, _)| h_id == &id));
    }
}
