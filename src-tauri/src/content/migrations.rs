use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, Transaction};

use crate::storage::error::{StorageError, StorageResult};
use crate::storage::migration::{ensure_schema, get_schema_version, set_schema_version, Migration};

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

struct DockCatalogRow {
    source_id: String,
    kind: String,
    saved: bool,
    retention_changed_at: String,
    inbox_position: Option<f64>,
    saved_position: Option<f64>,
    created_at: String,
    updated_at: String,
}

struct VaultCatalogRow {
    source_id: String,
    kind: String,
    created_at: String,
    updated_at: String,
}

pub fn ensure_content_schema(conn: &mut Connection, cleanup_days: i64) -> StorageResult<()> {
    let cleanup_delta = validate_cleanup_days(cleanup_days)?;
    ensure_schema(
        conn,
        &[Migration::new(
            3,
            "unified content catalog",
            CONTENT_SCHEMA_SQL,
        )],
    )?;

    let tx = conn.transaction()?;
    backfill_dock_catalog(&tx, cleanup_delta)?;
    backfill_vault_catalog(&tx)?;
    crate::content::projection::backfill_missing_projections(&tx)?;
    tx.commit()?;

    if get_schema_version(conn)? < 4 {
        let tx = conn.transaction()?;
        crate::content::projection::rebuild_vault_projections(&tx)?;
        set_schema_version(&tx, 4)?;
        tx.commit()?;
    }
    Ok(())
}

pub(crate) fn validate_cleanup_days(cleanup_days: i64) -> StorageResult<Duration> {
    if cleanup_days < 0 {
        return Err(StorageError::Validation(format!(
            "cleanup days cannot be negative: {cleanup_days}"
        )));
    }

    Duration::try_days(cleanup_days).ok_or_else(|| {
        StorageError::Validation(format!("cleanup days are out of range: {cleanup_days}"))
    })
}

fn backfill_dock_catalog(tx: &Transaction<'_>, cleanup_delta: Duration) -> StorageResult<()> {
    let rows = {
        let mut stmt = tx.prepare(
            r#"
            SELECT
                e.id,
                e.kind,
                n.entry_id IS NOT NULL AS saved,
                COALESCE(n.created_at, h.created_at, e.created_at) AS retention_changed_at,
                h.sort_order,
                n.sort_order,
                e.created_at,
                e.updated_at
            FROM entries e
            LEFT JOIN home_entries h ON h.entry_id = e.id
            LEFT JOIN note_entries n ON n.entry_id = e.id
            LEFT JOIN content_catalog c
                ON c.source = 'dock' AND c.source_id = e.id
            WHERE c.unified_id IS NULL
            ORDER BY e.id ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DockCatalogRow {
                source_id: row.get(0)?,
                kind: row.get(1)?,
                saved: row.get(2)?,
                retention_changed_at: row.get(3)?,
                inbox_position: row.get(4)?,
                saved_position: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    for row in rows {
        let retention_state = if row.saved { "saved" } else { "temporary" };
        let cleanup_at = if row.saved {
            None
        } else {
            Some(cleanup_at(&row.retention_changed_at, cleanup_delta)?)
        };
        tx.execute(
            r#"
            INSERT INTO content_catalog(
                unified_id, source, source_id, kind, retention_state,
                retention_changed_at, cleanup_at, inbox_position, saved_position,
                created_at, updated_at
            ) VALUES (
                ?1, 'dock', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10
            )
            "#,
            params![
                format!("dock:{}", row.source_id),
                row.source_id,
                row.kind,
                retention_state,
                row.retention_changed_at,
                cleanup_at,
                row.inbox_position,
                row.saved_position,
                row.created_at,
                row.updated_at,
            ],
        )?;
    }

    Ok(())
}

fn cleanup_at(retention_changed_at: &str, cleanup_delta: Duration) -> StorageResult<String> {
    let parsed = DateTime::parse_from_rfc3339(retention_changed_at).map_err(|error| {
        StorageError::Validation(format!(
            "invalid retention_changed_at {retention_changed_at:?}: {error}"
        ))
    })?;
    let cleanup_at = parsed.checked_add_signed(cleanup_delta).ok_or_else(|| {
        StorageError::Validation(format!(
            "cleanup timestamp is out of range for {retention_changed_at:?}"
        ))
    })?;
    if cleanup_delta.is_zero() {
        return Ok(retention_changed_at.to_string());
    }
    Ok(cleanup_at.with_timezone(&Utc).to_rfc3339())
}

fn backfill_vault_catalog(tx: &Transaction<'_>) -> StorageResult<()> {
    let rows = {
        let mut stmt = tx.prepare(
            r#"
            SELECT v.id, v.kind, v.created_at, v.updated_at
            FROM vault_entries v
            LEFT JOIN content_catalog c
                ON c.source = 'vault' AND c.source_id = v.id
            WHERE c.unified_id IS NULL
            ORDER BY v.updated_at DESC, v.id ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(VaultCatalogRow {
                source_id: row.get(0)?,
                kind: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    if rows.is_empty() {
        return Ok(());
    }

    let null_positions = tx.query_row(
        "SELECT COUNT(*)
         FROM content_catalog
         WHERE retention_state = 'saved' AND saved_position IS NULL",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if null_positions != 0 {
        return Err(StorageError::Validation(
            "cannot append Vault content after saved rows without positions".to_string(),
        ));
    }
    let mut saved_position = tx
        .query_row(
            "SELECT MAX(saved_position)
             FROM content_catalog
             WHERE retention_state = 'saved'",
            [],
            |row| row.get::<_, Option<f64>>(0),
        )?
        .unwrap_or(-1.0);
    if !saved_position.is_finite() {
        return Err(StorageError::Validation(
            "cannot append Vault content after a non-finite saved position".to_string(),
        ));
    }

    for row in rows {
        let next_position = saved_position + 1.0;
        if !next_position.is_finite() || next_position <= saved_position {
            return Err(StorageError::Validation(format!(
                "cannot represent a saved position after {saved_position}"
            )));
        }
        saved_position = next_position;
        tx.execute(
            r#"
            INSERT INTO content_catalog(
                unified_id, source, source_id, kind, retention_state,
                retention_changed_at, cleanup_at, inbox_position, saved_position,
                created_at, updated_at
            ) VALUES (
                ?1, 'vault', ?2, ?3, 'saved', ?4, NULL, NULL, ?5, ?4, ?6
            )
            "#,
            params![
                format!("vault:{}", row.source_id),
                row.source_id,
                row.kind,
                row.created_at,
                saved_position,
                row.updated_at,
            ],
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::types::Value;
    use rusqlite::{params, Connection};

    use super::{ensure_content_schema, CONTENT_SCHEMA_SQL};
    use crate::content::catalog::catalog_ids_for_scope;
    use crate::content::models::BrowseScope;
    use crate::scratchpad::storage::ensure_dock_schema;
    use crate::storage::error::StorageError;
    use crate::storage::migration::{
        ensure_schema, get_schema_version, set_schema_version, Migration,
    };
    use crate::vault::storage::ensure_vault_schema;

    type DockPayloadRow = (
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
    );
    type MembershipRow = (String, String, f64);
    type VaultPayloadRow = (String, String, String, Option<String>, String, String);

    #[derive(Debug, PartialEq)]
    struct LegacySnapshot {
        dock: Vec<DockPayloadRow>,
        home: Vec<MembershipRow>,
        note: Vec<MembershipRow>,
        vault: Vec<VaultPayloadRow>,
    }

    #[derive(Debug, PartialEq)]
    struct CatalogRow {
        unified_id: String,
        source: String,
        source_id: String,
        kind: String,
        retention_state: String,
        retention_changed_at: String,
        cleanup_at: Option<String>,
        inbox_position: Option<f64>,
        saved_position: Option<f64>,
        created_at: String,
        updated_at: String,
    }

    fn fixture_with_legacy_rows() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        ensure_dock_schema(&mut conn).unwrap();
        ensure_vault_schema(&mut conn).unwrap();

        let dock_rows = [
            (
                "home-only",
                "text",
                Some("temporary text"),
                None,
                "2026-07-01T08:00:00+00:00",
                "2026-07-08T08:00:00+00:00",
            ),
            (
                "note-file",
                "file",
                None,
                Some("C:/fixtures/report.pdf"),
                "2026-07-02T08:00:00+00:00",
                "2026-07-07T08:00:00+00:00",
            ),
            (
                "dual-member",
                "image",
                None,
                Some("C:/fixtures/photo.png"),
                "2026-07-03T08:00:00+00:00",
                "2026-07-06T08:00:00+00:00",
            ),
        ];
        for (id, kind, content, file_path, created_at, updated_at) in dock_rows {
            conn.execute(
                "INSERT INTO entries(
                    id, kind, content, file_path, source, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, 'fixture', ?5, ?6)",
                params![id, kind, content, file_path, created_at, updated_at],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO home_entries(entry_id, created_at, sort_order)
             VALUES ('home-only', '2026-07-04T08:00:00+00:00', 3.0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO note_entries(entry_id, created_at, sort_order)
             VALUES ('note-file', '2026-07-05T08:00:00+00:00', 0.5)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO home_entries(entry_id, created_at, sort_order)
             VALUES ('dual-member', '2026-07-04T09:00:00+00:00', 1.5)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO note_entries(entry_id, created_at, sort_order)
             VALUES ('dual-member', '2026-07-05T09:00:00+00:00', 2.5)",
            [],
        )
        .unwrap();

        let vault_rows = [
            (
                "credential-new",
                "New credential",
                "2026-07-01T10:00:00+00:00",
                "2026-07-10T10:00:00+00:00",
            ),
            (
                "credential-old",
                "Old credential",
                "2026-07-01T09:00:00+00:00",
                "2026-07-09T10:00:00+00:00",
            ),
        ];
        for (id, title, created_at, updated_at) in vault_rows {
            conn.execute(
                "INSERT INTO vault_entries(id, kind, title, notes, created_at, updated_at)
                 VALUES (?1, 'credential', ?2, 'fixture notes', ?3, ?4)",
                params![id, title, created_at, updated_at],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO vault_fields(
                id, entry_id, key, value, is_sensitive, sort_order
             ) VALUES
             ('fixture-user', 'credential-new', 'username', 'alice', 0, 0),
             ('fixture-password', 'credential-new', 'password', 'NeverIndexMe', 1, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vault_ai_metadata(
                entry_id, summary, search_aliases_json, content_hash, status
             ) VALUES (
                'credential-new', 'approved migration summary',
                '[\"migration alias\"]', 'fixture-hash', 'ready'
             )",
            [],
        )
        .unwrap();

        conn
    }

    fn legacy_snapshot(conn: &Connection) -> LegacySnapshot {
        let dock = conn
            .prepare(
                "SELECT id, kind, content, file_path, created_at, updated_at
                 FROM entries ORDER BY id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let memberships = |table: &str| {
            conn.prepare(&format!(
                "SELECT entry_id, created_at, sort_order FROM {table} ORDER BY entry_id"
            ))
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
        };
        let vault = conn
            .prepare(
                "SELECT id, kind, title, notes, created_at, updated_at
                 FROM vault_entries ORDER BY id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        LegacySnapshot {
            dock,
            home: memberships("home_entries"),
            note: memberships("note_entries"),
            vault,
        }
    }

    fn rows_snapshot(conn: &Connection, sql: &str) -> Vec<Vec<Value>> {
        let mut statement = conn.prepare(sql).unwrap();
        let column_count = statement.column_count();
        statement
            .query_map([], |row| {
                (0..column_count)
                    .map(|column| row.get(column))
                    .collect::<rusqlite::Result<Vec<Value>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    fn catalog_rows(conn: &Connection) -> Vec<CatalogRow> {
        conn.prepare(
            "SELECT unified_id, source, source_id, kind, retention_state,
                    retention_changed_at, cleanup_at, inbox_position, saved_position,
                    created_at, updated_at
             FROM content_catalog ORDER BY unified_id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(CatalogRow {
                unified_id: row.get(0)?,
                source: row.get(1)?,
                source_id: row.get(2)?,
                kind: row.get(3)?,
                retention_state: row.get(4)?,
                retention_changed_at: row.get(5)?,
                cleanup_at: row.get(6)?,
                inbox_position: row.get(7)?,
                saved_position: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    }

    fn object_exists(conn: &Connection, object_type: &str, name: &str) -> bool {
        conn.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2
             )",
            params![object_type, name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
            != 0
    }

    fn insert_vault_fixture(conn: &Connection, id: &str, updated_at: &str) {
        conn.execute(
            "INSERT INTO vault_entries(id, kind, title, notes, created_at, updated_at)
             VALUES (?1, 'credential', ?1, NULL, '2026-07-11T08:00:00+00:00', ?2)",
            params![id, updated_at],
        )
        .unwrap();
    }

    fn assert_saved_position_blocks_vault_append(position: Option<f64>) {
        let mut conn = fixture_with_legacy_rows();
        ensure_content_schema(&mut conn, 7).unwrap();
        conn.execute(
            "UPDATE content_catalog SET saved_position = ?1
             WHERE unified_id = 'dock:note-file'",
            params![position],
        )
        .unwrap();
        let before = catalog_rows(&conn);

        ensure_content_schema(&mut conn, 7).unwrap();
        assert_eq!(catalog_rows(&conn), before);

        insert_vault_fixture(&conn, "append-blocked", "2026-07-12T08:00:00+00:00");
        let error = ensure_content_schema(&mut conn, 7).unwrap_err();

        assert!(matches!(error, StorageError::Validation(_)));
        assert_eq!(catalog_rows(&conn), before);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM content_catalog
                 WHERE unified_id = 'vault:append-blocked'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn schema_and_backfill_map_legacy_rows_without_mutating_payloads() {
        let mut conn = fixture_with_legacy_rows();
        let before = legacy_snapshot(&conn);

        ensure_content_schema(&mut conn, 7).unwrap();

        assert_eq!(legacy_snapshot(&conn), before);
        assert_eq!(get_schema_version(&conn).unwrap(), 4);
        assert_eq!(
            conn.query_row(
                "SELECT revision FROM content_state WHERE singleton = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
        assert!(object_exists(&conn, "table", "content_pending_deletes"));
        assert_eq!(
            rows_snapshot(
                &conn,
                "SELECT name FROM pragma_table_info('content_pending_deletes') ORDER BY cid",
            ),
            vec![
                vec![Value::Text("token".into())],
                vec![Value::Text("unified_id".into())],
                vec![Value::Text("created_at".into())],
                vec![Value::Text("expires_at".into())],
                vec![Value::Text("status".into())],
            ]
        );
        assert!(object_exists(
            &conn,
            "index",
            "idx_content_pending_deletes_expiry"
        ));
        assert!(object_exists(&conn, "table", "content_fts"));
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_fts", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            5
        );
        let projected_text: String = conn
            .prepare(
                "SELECT title || ' ' || body || ' ' || tags || ' ' || aliases
                 FROM content_fts ORDER BY unified_id",
            )
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join(" ");
        assert!(projected_text.contains("alice"));
        assert!(projected_text.contains("migration alias"));
        assert!(!projected_text.contains("NeverIndexMe"));
        assert!(!projected_text.contains("C:/fixtures"));
        conn.execute(
            "INSERT INTO content_fts(unified_id, title, body, tags, aliases)
             VALUES ('probe', 'hello', '', '', '')",
            [],
        )
        .unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM content_fts WHERE content_fts MATCH 'hello'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        conn.execute("DELETE FROM content_fts WHERE unified_id = 'probe'", [])
            .unwrap();

        let rows = catalog_rows(&conn);
        assert_eq!(rows.len(), 5);
        assert_eq!(
            rows,
            vec![
                CatalogRow {
                    unified_id: "dock:dual-member".into(),
                    source: "dock".into(),
                    source_id: "dual-member".into(),
                    kind: "image".into(),
                    retention_state: "saved".into(),
                    retention_changed_at: "2026-07-05T09:00:00+00:00".into(),
                    cleanup_at: None,
                    inbox_position: Some(1.5),
                    saved_position: Some(2.5),
                    created_at: "2026-07-03T08:00:00+00:00".into(),
                    updated_at: "2026-07-06T08:00:00+00:00".into(),
                },
                CatalogRow {
                    unified_id: "dock:home-only".into(),
                    source: "dock".into(),
                    source_id: "home-only".into(),
                    kind: "text".into(),
                    retention_state: "temporary".into(),
                    retention_changed_at: "2026-07-04T08:00:00+00:00".into(),
                    cleanup_at: Some("2026-07-11T08:00:00+00:00".into()),
                    inbox_position: Some(3.0),
                    saved_position: None,
                    created_at: "2026-07-01T08:00:00+00:00".into(),
                    updated_at: "2026-07-08T08:00:00+00:00".into(),
                },
                CatalogRow {
                    unified_id: "dock:note-file".into(),
                    source: "dock".into(),
                    source_id: "note-file".into(),
                    kind: "file".into(),
                    retention_state: "saved".into(),
                    retention_changed_at: "2026-07-05T08:00:00+00:00".into(),
                    cleanup_at: None,
                    inbox_position: None,
                    saved_position: Some(0.5),
                    created_at: "2026-07-02T08:00:00+00:00".into(),
                    updated_at: "2026-07-07T08:00:00+00:00".into(),
                },
                CatalogRow {
                    unified_id: "vault:credential-new".into(),
                    source: "vault".into(),
                    source_id: "credential-new".into(),
                    kind: "credential".into(),
                    retention_state: "saved".into(),
                    retention_changed_at: "2026-07-01T10:00:00+00:00".into(),
                    cleanup_at: None,
                    inbox_position: None,
                    saved_position: Some(3.5),
                    created_at: "2026-07-01T10:00:00+00:00".into(),
                    updated_at: "2026-07-10T10:00:00+00:00".into(),
                },
                CatalogRow {
                    unified_id: "vault:credential-old".into(),
                    source: "vault".into(),
                    source_id: "credential-old".into(),
                    kind: "credential".into(),
                    retention_state: "saved".into(),
                    retention_changed_at: "2026-07-01T09:00:00+00:00".into(),
                    cleanup_at: None,
                    inbox_position: None,
                    saved_position: Some(4.5),
                    created_at: "2026-07-01T09:00:00+00:00".into(),
                    updated_at: "2026-07-09T10:00:00+00:00".into(),
                },
            ]
        );
        assert_eq!(
            catalog_ids_for_scope(&conn, BrowseScope::Saved).unwrap(),
            vec![
                "dock:note-file",
                "dock:dual-member",
                "vault:credential-new",
                "vault:credential-old",
            ]
        );
        assert_eq!(
            catalog_ids_for_scope(&conn, BrowseScope::Temporary).unwrap(),
            vec!["dock:home-only"]
        );
        assert_eq!(
            catalog_ids_for_scope(&conn, BrowseScope::All).unwrap(),
            vec![
                "vault:credential-new",
                "vault:credential-old",
                "dock:home-only",
                "dock:note-file",
                "dock:dual-member",
            ]
        );
    }

    #[test]
    fn upgrade_preserves_all_legacy_payload_counts_and_membership() {
        let mut conn = fixture_with_legacy_rows();
        conn.execute_batch(
            r#"
            INSERT INTO vault_entries(id, kind, title, notes, created_at, updated_at) VALUES
                ('bookmark-legacy', 'bookmark', 'Legacy bookmark', 'bookmark notes',
                 '2026-07-03T10:00:00+00:00', '2026-07-08T10:00:00+00:00'),
                ('note-legacy', 'note', 'Legacy note', 'note body',
                 '2026-07-04T10:00:00+00:00', '2026-07-07T10:00:00+00:00');
            INSERT INTO vault_fields(id, entry_id, key, value, is_sensitive, sort_order) VALUES
                ('bookmark-url', 'bookmark-legacy', 'url', 'https://fixture.invalid', 0, 0),
                ('credential-token', 'credential-old', ' token ', 'NeverIndexMeToken', 1, 0);
            INSERT INTO vault_tags(entry_id, tag, normalized_tag, source) VALUES
                ('credential-new', 'Manual Tag', 'manual tag', 'manual'),
                ('bookmark-legacy', 'AI Tag', 'ai tag', 'ai');
            INSERT INTO vault_ai_metadata(
                entry_id, summary, search_aliases_json, content_hash, status
            ) VALUES
                ('bookmark-legacy', 'safe approved summary', '["legacy portal"]',
                 'bookmark-hash', 'ready'),
                ('note-legacy', 'pending summary', '["pending alias"]',
                 'note-hash', 'pending');
            INSERT INTO vault_fts(entry_id, title, notes, searchable) VALUES
                ('credential-new', 'New credential', 'fixture notes', 'alice Manual Tag'),
                ('credential-old', 'Old credential', 'fixture notes', ''),
                ('bookmark-legacy', 'Legacy bookmark', 'bookmark notes',
                 'https://fixture.invalid AI Tag'),
                ('note-legacy', 'Legacy note', 'note body', '');
            "#,
        )
        .unwrap();

        let payload_tables = [
            ("entries", "SELECT * FROM entries ORDER BY id"),
            (
                "home_entries",
                "SELECT * FROM home_entries ORDER BY entry_id",
            ),
            (
                "note_entries",
                "SELECT * FROM note_entries ORDER BY entry_id",
            ),
            ("vault_entries", "SELECT * FROM vault_entries ORDER BY id"),
            (
                "vault_fields",
                "SELECT * FROM vault_fields ORDER BY entry_id, sort_order, id",
            ),
            (
                "vault_tags",
                "SELECT * FROM vault_tags ORDER BY entry_id, normalized_tag, source",
            ),
            (
                "vault_ai_metadata",
                "SELECT * FROM vault_ai_metadata ORDER BY entry_id",
            ),
        ];
        let before = payload_tables
            .iter()
            .map(|(table, sql)| (*table, rows_snapshot(&conn, sql)))
            .collect::<Vec<_>>();

        ensure_content_schema(&mut conn, 30).unwrap();

        for ((table, sql), (_, expected)) in payload_tables.iter().zip(&before) {
            assert_eq!(
                &rows_snapshot(&conn, sql),
                expected,
                "changed table: {table}"
            );
        }
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_catalog", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            7
        );
        assert_eq!(
            rows_snapshot(
                &conn,
                "SELECT unified_id, retention_state, inbox_position, saved_position
                 FROM content_catalog ORDER BY unified_id",
            ),
            vec![
                vec![
                    Value::Text("dock:dual-member".into()),
                    Value::Text("saved".into()),
                    1.5.into(),
                    2.5.into()
                ],
                vec![
                    Value::Text("dock:home-only".into()),
                    Value::Text("temporary".into()),
                    3.0.into(),
                    Value::Null
                ],
                vec![
                    Value::Text("dock:note-file".into()),
                    Value::Text("saved".into()),
                    Value::Null,
                    0.5.into()
                ],
                vec![
                    Value::Text("vault:bookmark-legacy".into()),
                    Value::Text("saved".into()),
                    Value::Null,
                    5.5.into()
                ],
                vec![
                    Value::Text("vault:credential-new".into()),
                    Value::Text("saved".into()),
                    Value::Null,
                    3.5.into()
                ],
                vec![
                    Value::Text("vault:credential-old".into()),
                    Value::Text("saved".into()),
                    Value::Null,
                    4.5.into()
                ],
                vec![
                    Value::Text("vault:note-legacy".into()),
                    Value::Text("saved".into()),
                    Value::Null,
                    6.5.into()
                ],
            ]
        );
        assert_eq!(
            rows_snapshot(
                &conn,
                "SELECT unified_id, COUNT(*) FROM content_fts
                 GROUP BY unified_id ORDER BY unified_id",
            ),
            rows_snapshot(
                &conn,
                "SELECT unified_id, 1 FROM content_catalog ORDER BY unified_id",
            )
        );
        assert_eq!(
            rows_snapshot(
                &conn,
                "SELECT entry_id, COUNT(*) FROM vault_fts
                 GROUP BY entry_id ORDER BY entry_id",
            ),
            rows_snapshot(&conn, "SELECT id, 1 FROM vault_entries ORDER BY id",)
        );

        let catalog_before_second_ensure =
            rows_snapshot(&conn, "SELECT * FROM content_catalog ORDER BY unified_id");
        let content_fts_before_second_ensure =
            rows_snapshot(&conn, "SELECT * FROM content_fts ORDER BY unified_id");
        let vault_fts_before_second_ensure =
            rows_snapshot(&conn, "SELECT * FROM vault_fts ORDER BY entry_id");
        ensure_content_schema(&mut conn, 30).unwrap();
        assert_eq!(
            rows_snapshot(&conn, "SELECT * FROM content_catalog ORDER BY unified_id"),
            catalog_before_second_ensure
        );
        assert_eq!(
            rows_snapshot(&conn, "SELECT * FROM content_fts ORDER BY unified_id"),
            content_fts_before_second_ensure
        );
        assert_eq!(
            rows_snapshot(&conn, "SELECT * FROM vault_fts ORDER BY entry_id"),
            vault_fts_before_second_ensure
        );
    }

    #[test]
    fn full_startup_repairs_v3_vault_flags_and_both_existing_fts_rows_once() {
        const SECRET: &str = "NeverIndexMe";
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        ensure_dock_schema(&mut conn).unwrap();
        ensure_vault_schema(&mut conn).unwrap();
        ensure_content_schema(&mut conn, 7).unwrap();
        conn.execute(
            "UPDATE vault_schema_version SET version=3 WHERE singleton=1",
            [],
        )
        .unwrap();
        set_schema_version(&conn, 3).unwrap();
        conn.execute_batch(
            r#"
            INSERT INTO vault_entries(
                id, kind, title, notes, created_at, updated_at
            ) VALUES (
                'legacy-v3', 'credential', 'NEVERINDEXME console',
                'notes neverindexme',
                '2026-07-01T00:00:00Z', '2026-07-01T00:00:00Z'
            );
            INSERT INTO vault_fields(
                id, entry_id, key, value, is_sensitive, sort_order
            ) VALUES
                ('legacy-password', 'legacy-v3', ' password ', 'NeverIndexMe', 0, 0),
                ('legacy-user', 'legacy-v3', 'username', 'alice', 0, 1);
            INSERT INTO vault_tags(entry_id, tag, normalized_tag, source) VALUES
                ('legacy-v3', 'NeVeRiNdExMe-tag', 'neverindexme-tag', 'manual'),
                ('legacy-v3', 'production', 'production', 'manual');
            INSERT INTO vault_ai_metadata(
                entry_id, summary, search_aliases_json, content_hash, status
            ) VALUES ('legacy-v3', 'pending neverindexme', '["NEVERINDEXME"]',
                      'legacy-hash', 'pending');
            INSERT INTO content_catalog(
                unified_id, source, source_id, kind, retention_state,
                retention_changed_at, cleanup_at, inbox_position, saved_position,
                created_at, updated_at
            ) VALUES (
                'vault:legacy-v3', 'vault', 'legacy-v3', 'credential', 'saved',
                '2026-07-01T00:00:00Z', NULL, NULL, 0.0,
                '2026-07-01T00:00:00Z', '2026-07-01T00:00:00Z'
            );
            INSERT INTO vault_fts(entry_id, title, notes, searchable) VALUES
                ('legacy-v3', 'NEVERINDEXME console', 'notes neverindexme',
                 'alice NeverIndexMe production');
            INSERT INTO content_fts(unified_id, title, body, tags, aliases) VALUES
                ('vault:legacy-v3', 'NEVERINDEXME console',
                 'notes neverindexme username alice',
                 'NeVeRiNdExMe-tag production', 'NEVERINDEXME');
            "#,
        )
        .unwrap();

        ensure_dock_schema(&mut conn).unwrap();
        ensure_vault_schema(&mut conn).unwrap();
        ensure_content_schema(&mut conn, 7).unwrap();

        let detail = crate::vault::storage::get_entry_detail(&conn, "legacy-v3").unwrap();
        assert!(
            detail
                .fields
                .iter()
                .find(|field| field.key == " password ")
                .unwrap()
                .is_sensitive
        );
        assert_eq!(
            detail.ai_metadata.unwrap().status,
            crate::vault::models::AiMetadataStatus::Pending
        );
        let vault_text: String = conn
            .query_row(
                "SELECT title || ' ' || notes || ' ' || searchable
                 FROM vault_fts WHERE entry_id='legacy-v3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let content_text: String = conn
            .query_row(
                "SELECT title || ' ' || body || ' ' || tags || ' ' || aliases
                 FROM content_fts WHERE unified_id='vault:legacy-v3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        for text in [&vault_text, &content_text] {
            assert!(!text.to_lowercase().contains(&SECRET.to_lowercase()));
            assert!(text.contains("alice"));
            assert!(text.contains("production"));
        }
        for table in ["vault_fts", "content_fts"] {
            let matches: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE {table} MATCH 'neverindexme'"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(matches, 0);
        }
        let before_revision: i64 = conn
            .query_row(
                "SELECT revision FROM content_state WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap();

        ensure_dock_schema(&mut conn).unwrap();
        ensure_vault_schema(&mut conn).unwrap();
        ensure_content_schema(&mut conn, 7).unwrap();

        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM vault_fts WHERE entry_id='legacy-v3'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM content_fts WHERE unified_id='vault:legacy-v3'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT revision FROM content_state WHERE singleton=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            before_revision
        );
    }

    #[test]
    fn content_v4_projection_failure_keeps_v3_marker_and_retries() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        ensure_dock_schema(&mut conn).unwrap();
        ensure_vault_schema(&mut conn).unwrap();
        ensure_content_schema(&mut conn, 7).unwrap();
        set_schema_version(&conn, 3).unwrap();
        insert_vault_fixture(&conn, "projection-retry", "2026-07-18T00:00:00Z");
        conn.execute(
            "INSERT INTO content_catalog(
                 unified_id, source, source_id, kind, retention_state,
                 retention_changed_at, cleanup_at, inbox_position, saved_position,
                 created_at, updated_at
             ) VALUES (
                 'vault:projection-retry', 'vault', 'projection-retry', 'credential',
                 'saved', '2026-07-11T08:00:00+00:00', NULL, NULL, 0.0,
                 '2026-07-11T08:00:00+00:00', '2026-07-18T00:00:00Z'
             )",
            [],
        )
        .unwrap();
        conn.execute_batch(
            r#"
            DROP TABLE content_fts;
            CREATE TABLE content_fts (
                unified_id TEXT NOT NULL,
                title TEXT NOT NULL CHECK (title = 'unsafe'),
                body TEXT NOT NULL,
                tags TEXT NOT NULL,
                aliases TEXT NOT NULL
            );
            INSERT INTO content_fts(unified_id, title, body, tags, aliases)
            VALUES ('vault:projection-retry', 'unsafe', '', '', '');
            "#,
        )
        .unwrap();

        assert!(ensure_content_schema(&mut conn, 7).is_err());
        assert_eq!(get_schema_version(&conn).unwrap(), 3);
        assert_eq!(
            conn.query_row(
                "SELECT title FROM content_fts WHERE unified_id='vault:projection-retry'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "unsafe"
        );
        conn.execute_batch(
            r#"
            DROP TABLE content_fts;
            CREATE VIRTUAL TABLE content_fts USING fts5(
                unified_id UNINDEXED, title, body, tags, aliases, tokenize = 'unicode61'
            );
            INSERT INTO content_fts(unified_id, title, body, tags, aliases)
            VALUES ('vault:projection-retry', 'unsafe', '', '', '');
            "#,
        )
        .unwrap();

        ensure_content_schema(&mut conn, 7).unwrap();

        assert_eq!(get_schema_version(&conn).unwrap(), 4);
        assert_eq!(
            conn.query_row(
                "SELECT title FROM content_fts WHERE unified_id='vault:projection-retry'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "projection-retry"
        );
    }

    #[test]
    fn repeated_backfill_inserts_only_missing_rows_and_preserves_positions() {
        let mut conn = fixture_with_legacy_rows();
        ensure_content_schema(&mut conn, 7).unwrap();
        let first_rows = catalog_rows(&conn);
        conn.execute(
            "UPDATE content_fts SET title = 'preserved projection'
             WHERE unified_id = 'dock:home-only'",
            [],
        )
        .unwrap();

        conn.execute(
            "UPDATE home_entries SET sort_order = 99.0 WHERE entry_id = 'home-only'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE vault_entries
             SET updated_at = '2026-07-18T00:00:00+00:00'
             WHERE id = 'credential-old'",
            [],
        )
        .unwrap();
        ensure_content_schema(&mut conn, 30).unwrap();

        assert_eq!(catalog_rows(&conn), first_rows);
        assert_eq!(
            conn.query_row(
                "SELECT title FROM content_fts WHERE unified_id = 'dock:home-only'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "preserved projection"
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_catalog", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            5
        );
    }

    #[test]
    fn later_vault_rows_append_after_existing_saved_order_in_stable_source_order() {
        let mut conn = fixture_with_legacy_rows();
        ensure_content_schema(&mut conn, 7).unwrap();
        let old_ids = catalog_ids_for_scope(&conn, BrowseScope::Saved).unwrap();

        insert_vault_fixture(&conn, "credential-later-b", "2026-07-11T09:00:00+00:00");
        insert_vault_fixture(&conn, "credential-latest", "2026-07-12T09:00:00+00:00");
        insert_vault_fixture(&conn, "credential-later-a", "2026-07-11T09:00:00+00:00");

        ensure_content_schema(&mut conn, 7).unwrap();

        let new_ids = catalog_ids_for_scope(&conn, BrowseScope::Saved).unwrap();
        assert!(new_ids.starts_with(&old_ids));
        assert_eq!(
            &new_ids[old_ids.len()..],
            &[
                "vault:credential-latest",
                "vault:credential-later-a",
                "vault:credential-later-b",
            ]
        );
    }

    #[test]
    fn null_saved_position_blocks_vault_append_without_reordering_existing_rows() {
        assert_saved_position_blocks_vault_append(None);
    }

    #[test]
    fn infinite_saved_position_blocks_vault_append_without_partial_rows() {
        assert_saved_position_blocks_vault_append(Some(f64::INFINITY));
    }

    #[test]
    fn imprecise_saved_position_successor_blocks_vault_append_without_partial_rows() {
        assert_saved_position_blocks_vault_append(Some(9_007_199_254_740_992.0));
    }

    #[test]
    fn zero_cleanup_days_makes_temporary_rows_due_at_retention_change() {
        let mut conn = fixture_with_legacy_rows();

        ensure_content_schema(&mut conn, 0).unwrap();

        let (retention_changed_at, cleanup_at): (String, Option<String>) = conn
            .query_row(
                "SELECT retention_changed_at, cleanup_at
                 FROM content_catalog WHERE unified_id = 'dock:home-only'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(cleanup_at.as_deref(), Some(retention_changed_at.as_str()));
    }

    #[test]
    fn invalid_cleanup_days_are_rejected_before_schema_migration() {
        for cleanup_days in [-1, i64::MAX] {
            let mut conn = fixture_with_legacy_rows();
            let before = legacy_snapshot(&conn);

            let error = ensure_content_schema(&mut conn, cleanup_days).unwrap_err();

            assert!(matches!(error, StorageError::Validation(_)));
            assert_eq!(get_schema_version(&conn).unwrap(), 2);
            assert!(!object_exists(&conn, "table", "content_catalog"));
            assert_eq!(legacy_snapshot(&conn), before);
        }
    }

    #[test]
    fn zero_cleanup_days_reject_invalid_timestamp_without_partial_backfill() {
        let mut conn = fixture_with_legacy_rows();
        conn.execute(
            "UPDATE home_entries SET created_at = 'not-rfc3339'
             WHERE entry_id = 'home-only'",
            [],
        )
        .unwrap();
        let before = legacy_snapshot(&conn);

        let error = ensure_content_schema(&mut conn, 0).unwrap_err();

        assert!(matches!(error, StorageError::Validation(_)));
        assert_eq!(get_schema_version(&conn).unwrap(), 3);
        assert_eq!(legacy_snapshot(&conn), before);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_catalog", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
    }

    #[test]
    fn failed_backfill_rolls_back_rows_but_keeps_committed_schema_migration() {
        let mut conn = fixture_with_legacy_rows();
        ensure_schema(
            &mut conn,
            &[Migration::new(
                3,
                "unified content catalog",
                CONTENT_SCHEMA_SQL,
            )],
        )
        .unwrap();
        conn.execute_batch(
            r#"
            CREATE TRIGGER fail_vault_catalog_backfill
            BEFORE INSERT ON content_catalog
            WHEN NEW.source = 'vault'
            BEGIN
                SELECT RAISE(ABORT, 'forced vault catalog failure');
            END;
            "#,
        )
        .unwrap();

        assert!(ensure_content_schema(&mut conn, 7).is_err());

        assert_eq!(get_schema_version(&conn).unwrap(), 3);
        assert!(object_exists(&conn, "table", "content_catalog"));
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_catalog", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_fts", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
    }

    #[test]
    fn failed_projection_backfill_rolls_back_catalog_and_all_new_projection_rows() {
        let mut conn = fixture_with_legacy_rows();
        ensure_schema(
            &mut conn,
            &[Migration::new(
                3,
                "unified content catalog",
                CONTENT_SCHEMA_SQL,
            )],
        )
        .unwrap();
        conn.execute_batch(
            r#"
            DROP TABLE content_fts;
            CREATE TABLE content_fts (
                unified_id TEXT NOT NULL
                    CHECK (unified_id != 'dock:note-file'),
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                tags TEXT NOT NULL,
                aliases TEXT NOT NULL
            );
            "#,
        )
        .unwrap();

        assert!(ensure_content_schema(&mut conn, 7).is_err());

        assert_eq!(get_schema_version(&conn).unwrap(), 3);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_catalog", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM content_fts", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
    }
}
