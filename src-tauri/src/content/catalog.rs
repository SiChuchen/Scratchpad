use rusqlite::{params, Connection, OptionalExtension};

use crate::content::models::{
    BrowseScope, ContentCapabilities, ContentKind, ContentSummary, RetentionState, UnifiedContentId,
};
use crate::content::projection::normalize_text;
use crate::storage::error::{StorageError, StorageResult};

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

    use super::{catalog_ids_for_scope, summary_by_id};
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
}
