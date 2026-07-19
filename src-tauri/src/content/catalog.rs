use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::content::models::{
    BrowseScope, ContentCapabilities, ContentKind, ContentSource, ContentSummary, RetentionState,
    UnifiedContentId,
};
use crate::content::projection::normalize_text;
use crate::storage::error::{StorageError, StorageResult};

#[derive(Debug, Clone)]
pub(crate) struct CatalogEntry {
    pub source: ContentSource,
    pub source_id: String,
    pub kind: ContentKind,
    pub retention: RetentionState,
    pub cleanup_at: Option<String>,
}

pub fn current_revision(conn: &Connection) -> StorageResult<i64> {
    Ok(conn.query_row(
        "SELECT revision FROM content_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?)
}

pub(crate) fn bump_revision(conn: &Connection) -> StorageResult<i64> {
    Ok(conn.query_row(
        "UPDATE content_state SET revision = revision + 1
         WHERE singleton = 1 RETURNING revision",
        [],
        |row| row.get(0),
    )?)
}

pub(crate) fn top_position(conn: &Connection, retention: RetentionState) -> StorageResult<f64> {
    let (state, column) = match retention {
        RetentionState::Temporary => ("temporary", "inbox_position"),
        RetentionState::Saved => ("saved", "saved_position"),
    };
    let (total, positioned, minimum): (i64, i64, Option<f64>) = conn.query_row(
        &format!(
            "SELECT COUNT(*), COUNT({column}), MIN({column})
             FROM content_catalog WHERE retention_state = ?1"
        ),
        params![state],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if total == 0 {
        return Ok(0.0);
    }
    if positioned != total {
        return Err(StorageError::Validation(format!(
            "cannot place content above {state} rows without positions"
        )));
    }
    let minimum = minimum
        .ok_or_else(|| StorageError::Validation(format!("cannot read the top {state} position")))?;
    if !minimum.is_finite() {
        return Err(StorageError::Validation(format!(
            "cannot place content above a non-finite {state} position"
        )));
    }
    let next = minimum - 1.0;
    if !next.is_finite() || next >= minimum {
        return Err(StorageError::Validation(format!(
            "cannot represent a {state} position before {minimum}"
        )));
    }
    Ok(next)
}

pub(crate) fn catalog_entry_by_id(
    conn: &Connection,
    unified_id: &str,
) -> StorageResult<CatalogEntry> {
    UnifiedContentId::parse(unified_id).map_err(StorageError::Validation)?;
    let row = conn
        .query_row(
            "SELECT source, source_id, kind, retention_state, retention_changed_at,
                    cleanup_at, created_at, updated_at
             FROM content_catalog WHERE unified_id = ?1",
            params![unified_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Validation(format!("content catalog row not found: {unified_id}"))
        })?;
    let (
        source,
        source_id,
        kind,
        retention,
        retention_changed_at,
        cleanup_at,
        created_at,
        updated_at,
    ) = row;
    let source = parse_source(&source, unified_id)?;
    let expected_id =
        UnifiedContentId::new(source, &source_id).map_err(StorageError::Validation)?;
    if expected_id.as_str() != unified_id {
        return Err(StorageError::Validation(format!(
            "content catalog identity mismatch for {unified_id}"
        )));
    }
    parse_timestamp(&retention_changed_at, "retention_changed_at", unified_id)?;
    parse_timestamp(&created_at, "created_at", unified_id)?;
    parse_timestamp(&updated_at, "updated_at", unified_id)?;
    if let Some(cleanup_at) = cleanup_at.as_deref() {
        parse_timestamp(cleanup_at, "cleanup_at", unified_id)?;
    }
    Ok(CatalogEntry {
        source,
        source_id,
        kind: parse_kind(&kind, unified_id)?,
        retention: parse_retention(&retention, unified_id)?,
        cleanup_at,
    })
}

pub(crate) fn parse_timestamp(
    value: &str,
    field: &str,
    unified_id: &str,
) -> StorageResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| {
            StorageError::Validation(format!("invalid {field} for {unified_id}: {error}"))
        })
}

pub(crate) fn parse_source(value: &str, unified_id: &str) -> StorageResult<ContentSource> {
    ContentSource::parse(value).ok_or_else(|| {
        StorageError::Validation(format!("unknown content source for {unified_id}: {value}"))
    })
}

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

pub fn summary_by_id(
    conn: &Connection,
    unified_id: &str,
    reorderable: bool,
) -> StorageResult<ContentSummary> {
    UnifiedContentId::parse(unified_id).map_err(StorageError::Validation)?;
    let catalog = conn
        .query_row(
            "SELECT kind, retention_state, created_at, updated_at, cleanup_at
             FROM content_catalog WHERE unified_id = ?1",
            params![unified_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Validation(format!("content catalog row not found: {unified_id}"))
        })?;
    let projection_count = conn.query_row(
        "SELECT COUNT(*) FROM content_fts WHERE unified_id = ?1",
        params![unified_id],
        |row| row.get::<_, i64>(0),
    )?;
    if projection_count != 1 {
        return Err(StorageError::Validation(format!(
            "expected one safe projection for {unified_id}, found {projection_count}"
        )));
    }
    let projection = conn.query_row(
        "SELECT title, body, tags, aliases
         FROM content_fts WHERE unified_id = ?1",
        params![unified_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )?;
    let (kind, retention_state, created_at, updated_at, cleanup_at) = catalog;
    let kind = parse_kind(&kind, unified_id)?;
    let retention = parse_retention(&retention_state, unified_id)?;
    let (title, body, tags, aliases) = projection;

    Ok(ContentSummary {
        id: unified_id.to_string(),
        kind,
        retention,
        title,
        preview: preview_from_projection(&body, &tags, &aliases),
        created_at,
        updated_at,
        cleanup_at,
        capabilities: ContentCapabilities::for_item(kind, retention, reorderable),
    })
}

pub fn summaries_for_scope(
    conn: &Connection,
    scope: BrowseScope,
    reorderable: bool,
) -> StorageResult<Vec<ContentSummary>> {
    catalog_ids_for_scope(conn, scope)?
        .into_iter()
        .map(|id| summary_by_id(conn, &id, reorderable))
        .collect()
}

pub(crate) fn parse_kind(value: &str, unified_id: &str) -> StorageResult<ContentKind> {
    match value {
        "text" => Ok(ContentKind::Text),
        "image" => Ok(ContentKind::Image),
        "file" => Ok(ContentKind::File),
        "credential" => Ok(ContentKind::Credential),
        "bookmark" => Ok(ContentKind::Bookmark),
        "note" => Ok(ContentKind::Note),
        _ => Err(StorageError::Validation(format!(
            "unknown content kind for {unified_id}: {value}"
        ))),
    }
}

pub(crate) fn parse_retention(value: &str, unified_id: &str) -> StorageResult<RetentionState> {
    match value {
        "temporary" => Ok(RetentionState::Temporary),
        "saved" => Ok(RetentionState::Saved),
        _ => Err(StorageError::Validation(format!(
            "unknown retention state for {unified_id}: {value}"
        ))),
    }
}

fn preview_from_projection(body: &str, tags: &str, aliases: &str) -> Option<String> {
    let preview = normalize_text(&[body, tags, aliases].join(" "));
    if preview.is_empty() {
        None
    } else {
        Some(preview.chars().take(160).collect())
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::{params, Connection};

    use super::{
        bump_revision, catalog_ids_for_scope, current_revision, summary_by_id, top_position,
    };
    use crate::content::migrations::ensure_content_schema;
    use crate::content::models::{BrowseScope, ContentKind, RetentionState};
    use crate::content::projection::delete_projection;
    use crate::content::projection::tests::{fixture_with_all_kinds, SENSITIVE_LITERAL};
    use crate::storage::error::StorageError;

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

    #[test]
    fn summaries_cover_all_kinds_and_retentions_with_exact_capabilities() {
        let conn = fixture_with_all_kinds();
        let cases = [
            ("dock:text-1", ContentKind::Text, RetentionState::Temporary),
            (
                "dock:image-1",
                ContentKind::Image,
                RetentionState::Temporary,
            ),
            ("dock:file-1", ContentKind::File, RetentionState::Saved),
            (
                "vault:credential-1",
                ContentKind::Credential,
                RetentionState::Saved,
            ),
            (
                "vault:bookmark-1",
                ContentKind::Bookmark,
                RetentionState::Saved,
            ),
            ("vault:note-1", ContentKind::Note, RetentionState::Saved),
        ];

        for (id, kind, retention) in cases {
            let search_summary = summary_by_id(&conn, id, false).unwrap();
            assert_eq!(search_summary.id, id);
            assert_eq!(search_summary.kind, kind);
            assert_eq!(search_summary.retention, retention);
            assert!(!search_summary.capabilities.reorder);

            let browse_summary = summary_by_id(&conn, id, true).unwrap();
            assert!(browse_summary.capabilities.reorder);
            assert_eq!(
                browse_summary.capabilities,
                crate::content::models::ContentCapabilities::for_item(kind, retention, true)
            );
        }
    }

    #[test]
    fn summary_uses_only_safe_projection_and_unicode_bounded_preview() {
        let mut conn = fixture_with_all_kinds();
        let long_body = format!("安全\u{0000}内容{}", "汉".repeat(200));
        conn.execute(
            "INSERT INTO entries(
                id, kind, content, title, source, created_at, updated_at
             ) VALUES ('preview-long', 'text', ?1, 'Visible title', 'fixture',
                       '2026-07-15T08:00:00+00:00', '2026-07-15T08:00:00+00:00')",
            params![long_body],
        )
        .unwrap();
        ensure_content_schema(&mut conn, 7).unwrap();

        let summary = summary_by_id(&conn, "dock:preview-long", false).unwrap();
        let preview = summary.preview.unwrap();
        assert_eq!(summary.title, "Visible title");
        assert!(preview.chars().count() <= 160);
        assert!(!preview.chars().any(char::is_control));
        assert!(preview.starts_with("安全内容"));

        let credential = summary_by_id(&conn, "vault:credential-1", false).unwrap();
        let visible = serde_json::to_string(&credential).unwrap();
        assert!(!visible.contains(SENSITIVE_LITERAL));
        assert!(!visible.contains("PrivateAssetPath"));
    }

    #[test]
    fn summary_rejects_missing_projection_and_unknown_catalog_values() {
        let conn = fixture_with_all_kinds();
        delete_projection(&conn, "dock:text-1").unwrap();
        assert!(matches!(
            summary_by_id(&conn, "dock:text-1", false).unwrap_err(),
            StorageError::Validation(_)
        ));

        conn.pragma_update(None, "ignore_check_constraints", "ON")
            .unwrap();
        conn.execute(
            "UPDATE content_catalog SET kind = 'unknown' WHERE unified_id = 'dock:image-1'",
            [],
        )
        .unwrap();
        assert!(matches!(
            summary_by_id(&conn, "dock:image-1", false).unwrap_err(),
            StorageError::Validation(_)
        ));

        conn.execute(
            "UPDATE content_catalog SET retention_state = 'unknown'
             WHERE unified_id = 'dock:file-1'",
            [],
        )
        .unwrap();
        assert!(matches!(
            summary_by_id(&conn, "dock:file-1", false).unwrap_err(),
            StorageError::Validation(_)
        ));
    }

    #[test]
    fn revision_helpers_read_and_bump_the_singleton_once() {
        let conn = fixture_with_all_kinds();

        assert_eq!(current_revision(&conn).unwrap(), 0);
        assert_eq!(bump_revision(&conn).unwrap(), 1);
        assert_eq!(current_revision(&conn).unwrap(), 1);
        assert_eq!(bump_revision(&conn).unwrap(), 2);
        assert_eq!(current_revision(&conn).unwrap(), 2);
    }

    #[test]
    fn top_position_starts_at_zero_and_is_strictly_above_the_current_top() {
        let conn = fixture_with_all_kinds();
        conn.execute(
            "DELETE FROM content_catalog WHERE retention_state='temporary'",
            [],
        )
        .unwrap();
        assert_eq!(top_position(&conn, RetentionState::Temporary).unwrap(), 0.0);

        conn.execute(
            "DELETE FROM content_catalog
             WHERE retention_state='saved' AND unified_id<>'dock:file-1'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE content_catalog SET saved_position=10.0
             WHERE unified_id='dock:file-1'",
            [],
        )
        .unwrap();
        assert_eq!(top_position(&conn, RetentionState::Saved).unwrap(), 9.0);
    }

    #[test]
    fn top_position_rejects_null_non_finite_and_saturated_values() {
        for position in [None, Some(f64::INFINITY), Some(-9_007_199_254_740_992.0)] {
            let conn = fixture_with_all_kinds();
            conn.execute(
                "DELETE FROM content_catalog WHERE retention_state='temporary'",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE content_catalog
                 SET retention_state='temporary', inbox_position=?1, saved_position=NULL
                 WHERE unified_id='dock:file-1'",
                params![position],
            )
            .unwrap();

            assert!(matches!(
                top_position(&conn, RetentionState::Temporary),
                Err(StorageError::Validation(_))
            ));
        }
    }
}
