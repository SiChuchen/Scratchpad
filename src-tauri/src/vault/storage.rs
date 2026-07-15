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
}
