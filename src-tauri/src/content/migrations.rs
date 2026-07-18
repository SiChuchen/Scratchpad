use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, Transaction};

use crate::storage::error::{StorageError, StorageResult};
use crate::storage::migration::{ensure_schema, Migration};

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
    ensure_schema(
        conn,
        &[Migration::new(
            3,
            "unified content catalog",
            CONTENT_SCHEMA_SQL,
        )],
    )?;

    let cleanup_delta = Duration::try_days(cleanup_days.max(0)).ok_or_else(|| {
        StorageError::Validation(format!("cleanup days are out of range: {cleanup_days}"))
    })?;
    let tx = conn.transaction()?;
    backfill_dock_catalog(&tx, cleanup_delta)?;
    backfill_vault_catalog(&tx)?;
    tx.commit()?;
    Ok(())
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
    if cleanup_delta.is_zero() {
        return Ok(retention_changed_at.to_string());
    }

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
    Ok(cleanup_at.with_timezone(&Utc).to_rfc3339())
}

fn backfill_vault_catalog(tx: &Transaction<'_>) -> StorageResult<()> {
    let mut saved_position = tx
        .query_row(
            "SELECT MAX(saved_position)
             FROM content_catalog
             WHERE retention_state = 'saved'",
            [],
            |row| row.get::<_, Option<f64>>(0),
        )?
        .unwrap_or(-1.0);
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

    for row in rows {
        saved_position += 1.0;
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
    use rusqlite::{params, Connection};

    use super::{ensure_content_schema, CONTENT_SCHEMA_SQL};
    use crate::content::catalog::catalog_ids_for_scope;
    use crate::content::models::BrowseScope;
    use crate::scratchpad::storage::ensure_dock_schema;
    use crate::storage::migration::{ensure_schema, get_schema_version, Migration};
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
        ensure_dock_schema(&mut conn, 7).unwrap();
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

    #[test]
    fn schema_and_backfill_map_legacy_rows_without_mutating_payloads() {
        let mut conn = fixture_with_legacy_rows();
        let before = legacy_snapshot(&conn);

        ensure_content_schema(&mut conn, 7).unwrap();

        assert_eq!(legacy_snapshot(&conn), before);
        assert_eq!(get_schema_version(&conn).unwrap(), 3);
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
            0
        );
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
    fn repeated_backfill_inserts_only_missing_rows_and_preserves_positions() {
        let mut conn = fixture_with_legacy_rows();
        ensure_content_schema(&mut conn, 7).unwrap();
        let first_rows = catalog_rows(&conn);

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
            conn.query_row("SELECT COUNT(*) FROM content_catalog", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            5
        );
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
}
