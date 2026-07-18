use rusqlite::Connection;

use crate::content::models::BrowseScope;
use crate::storage::error::StorageResult;

pub fn catalog_ids_for_scope(conn: &Connection, scope: BrowseScope) -> StorageResult<Vec<String>> {
    let sql = match scope {
        BrowseScope::Temporary => {
            r#"
            SELECT unified_id
            FROM content_catalog
            WHERE retention_state = 'temporary'
            ORDER BY
                inbox_position IS NULL ASC,
                inbox_position ASC,
                updated_at DESC,
                unified_id ASC
            "#
        }
        BrowseScope::Saved => {
            r#"
            SELECT unified_id
            FROM content_catalog
            WHERE retention_state = 'saved'
            ORDER BY
                saved_position IS NULL ASC,
                saved_position ASC,
                updated_at DESC,
                unified_id ASC
            "#
        }
        BrowseScope::All => {
            r#"
            SELECT unified_id
            FROM content_catalog
            ORDER BY updated_at DESC, unified_id ASC
            "#
        }
    };

    let mut stmt = conn.prepare(sql)?;
    let ids = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use rusqlite::{params, Connection};

    use super::catalog_ids_for_scope;
    use crate::content::models::BrowseScope;

    fn catalog_fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE content_catalog (
                unified_id TEXT PRIMARY KEY,
                retention_state TEXT NOT NULL,
                inbox_position REAL,
                saved_position REAL,
                updated_at TEXT NOT NULL
            );
            "#,
        )
        .unwrap();

        let rows = [
            (
                "dock:temp-null",
                "temporary",
                None,
                None,
                "2026-07-18T12:00:00+00:00",
            ),
            (
                "dock:temp-a",
                "temporary",
                Some(1.0),
                None,
                "2026-07-18T10:00:00+00:00",
            ),
            (
                "dock:temp-b",
                "temporary",
                Some(1.0),
                None,
                "2026-07-18T11:00:00+00:00",
            ),
            (
                "dock:saved-null",
                "saved",
                None,
                None,
                "2026-07-18T09:00:00+00:00",
            ),
            (
                "dock:saved-b",
                "saved",
                None,
                Some(2.0),
                "2026-07-18T08:00:00+00:00",
            ),
            (
                "dock:saved-a",
                "saved",
                None,
                Some(2.0),
                "2026-07-18T08:00:00+00:00",
            ),
        ];
        for (id, retention, inbox, saved, updated_at) in rows {
            conn.execute(
                "INSERT INTO content_catalog(
                    unified_id, retention_state, inbox_position, saved_position, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, retention, inbox, saved, updated_at],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn scoped_catalog_ids_use_explicit_null_last_and_stable_tie_breaks() {
        let conn = catalog_fixture();

        assert_eq!(
            catalog_ids_for_scope(&conn, BrowseScope::Temporary).unwrap(),
            vec!["dock:temp-b", "dock:temp-a", "dock:temp-null"]
        );
        assert_eq!(
            catalog_ids_for_scope(&conn, BrowseScope::Saved).unwrap(),
            vec!["dock:saved-a", "dock:saved-b", "dock:saved-null"]
        );
    }

    #[test]
    fn all_scope_uses_updated_time_then_id_for_deterministic_order() {
        let conn = catalog_fixture();

        assert_eq!(
            catalog_ids_for_scope(&conn, BrowseScope::All).unwrap(),
            vec![
                "dock:temp-null",
                "dock:temp-b",
                "dock:temp-a",
                "dock:saved-null",
                "dock:saved-a",
                "dock:saved-b",
            ]
        );
    }
}
