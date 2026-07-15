use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use rusqlite::{params, Connection, Row};

use crate::storage::error::{StorageError, StorageResult};
use crate::vault::models::{
    EntryKind, FieldInput, VaultEntry, VaultEntryDetail, VaultEntryInput, VaultField,
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
}
