use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use rusqlite::{params, Connection, Row};

use crate::storage::error::{StorageError, StorageResult};
use crate::vault::models::{
    AiMetadataStatus, EntryKind, FieldInput, TagSource, VaultAiMetadata, VaultEntry,
    VaultEntryDetail, VaultEntryInput, VaultField, VaultTag, is_default_sensitive_key,
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

pub fn create_entry(conn: &mut Connection, input: &VaultEntryInput) -> StorageResult<VaultEntry> {
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
    tx.commit()?;
    fts5_upsert(conn, &id)?;
    get_entry_by_id(conn, &id)?.ok_or_else(|| StorageError::Other("insert failed".into()))
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
) -> StorageResult<VaultEntry> {
    let tx = conn.transaction()?;
    let now = now_rfc3339();
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
    tx.commit()?;
    fts5_upsert(conn, id)?;
    get_entry_by_id(conn, id)?.ok_or_else(|| StorageError::Other("missing after update".into()))
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

pub fn list_entries(conn: &Connection, kind: Option<EntryKind>) -> StorageResult<Vec<VaultEntry>> {
    let mut stmt = if let Some(k) = kind {
        let mut s = conn.prepare(
            "SELECT id, kind, title, notes, created_at, updated_at
             FROM vault_entries WHERE kind=?1 ORDER BY updated_at DESC",
        )?;
        let rows = s.query_map(params![k.as_str()], row_to_entry)?;
        return rows.collect::<rusqlite::Result<Vec<_>>>().map_err(StorageError::from);
    } else {
        conn.prepare(
            "SELECT id, kind, title, notes, created_at, updated_at
             FROM vault_entries ORDER BY updated_at DESC",
        )?
    };
    let rows = stmt.query_map([], row_to_entry)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(StorageError::from)
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

pub fn set_tags(conn: &mut Connection, entry_id: &str, tags: &[String]) -> StorageResult<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM vault_tags WHERE entry_id=?1 AND source='manual'",
        params![entry_id],
    )?;
    for t in tags {
        // 用户手动设置的标签走 manual 来源；归一化失败（空/纯空白）的标签跳过
        if let Some(norm) = crate::vault::migrations::normalize_tag(t) {
            let display = t.trim().to_string();
            tx.execute(
                "INSERT OR IGNORE INTO vault_tags(entry_id, tag, normalized_tag, source)
                 VALUES (?1, ?2, ?3, 'manual')",
                params![entry_id, display, norm],
            )?;
        }
    }
    tx.commit()?;
    fts5_upsert(conn, entry_id)?;
    Ok(())
}

pub fn list_tags(conn: &Connection, entry_id: &str) -> StorageResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT tag FROM vault_tags WHERE entry_id=?1 ORDER BY tag")?;
    let rows = stmt.query_map(params![entry_id], |r| r.get::<_, String>(0))?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(StorageError::from)
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let entry = create_entry(&mut conn, &input).unwrap();
        assert_eq!(entry.title, "Prod DB");

        let fields = list_fields(&conn, &entry.id).unwrap();
        assert_eq!(fields.len(), 2);
        // 'password' 字段应被自动标记为 sensitive
        let pwd = fields.iter().find(|f| f.key == "password").unwrap();
        assert!(pwd.is_sensitive, "password should default to sensitive");
        let user = fields.iter().find(|f| f.key == "user").unwrap();
        assert!(!user.is_sensitive);
    }

    #[test]
    fn create_entry_rejects_unknown_kind() {
        let mut conn = open_test_db();
        // 直接 SQL 注入非法 kind 来测试 CHECK 约束
        let result = conn.execute(
            "INSERT INTO vault_entries(id, kind, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params!["v1", "bogus", "x", "t", "t"],
        );
        assert!(result.is_err());
    }

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
    }

    #[test]
    fn update_entry_replaces_fields_and_bumps_updated_at() {
        let mut conn = open_test_db();
        let e = make_entry(&mut conn, "Original");
        let original_updated = e.updated_at.clone();

        std::thread::sleep(std::time::Duration::from_millis(10));

        update_entry(
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

        let detail = get_entry_detail(&conn, &e.id).unwrap();
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
        set_tags(&mut conn, &e.id, &["t1".into(), "t2".into()]).unwrap();
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
        assert_eq!(entries[0].title, "B"); // newer first
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
        assert_eq!(bm[0].title, "BM");
    }

    #[test]
    fn set_tags_replaces_existing() {
        let mut conn = open_test_db();
        let e = make_entry(&mut conn, "T");
        set_tags(&mut conn, &e.id, &["a".into()]).unwrap();
        set_tags(&mut conn, &e.id, &["b".into(), "c".into()]).unwrap();
        let mut tags = list_tags(&conn, &e.id).unwrap();
        tags.sort();
        assert_eq!(tags, vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn fts5_indexes_title_username_and_tags_not_password() {
        let mut conn = open_test_db();
        let e = create_entry(
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
        set_tags(&mut conn, &e.id, &["mysql".into(), "prod".into()]).unwrap();

        // 搜 title
        let hits = fts5_search(&conn, "production", 10).unwrap();
        assert!(hits.iter().any(|(id, _)| id == &e.id));

        // 搜 username
        let hits = fts5_search(&conn, "admin", 10).unwrap();
        assert!(hits.iter().any(|(id, _)| id == &e.id));

        // 搜 tag
        let hits = fts5_search(&conn, "mysql", 10).unwrap();
        assert!(hits.iter().any(|(id, _)| id == &e.id));

        // 不能搜 password
        let hits = fts5_search(&conn, "supersecretvalue", 10).unwrap();
        assert!(!hits.iter().any(|(id, _)| id == &e.id));
    }

    #[test]
    fn fts5_search_after_delete_returns_nothing() {
        let mut conn = open_test_db();
        let e = make_entry(&mut conn, "DeleteMe");
        assert!(!fts5_search(&conn, "DeleteMe", 10).unwrap().is_empty());
        delete_entry(&mut conn, &e.id).unwrap();
        assert!(fts5_search(&conn, "DeleteMe", 10).unwrap().is_empty());
    }
}
