use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};

use crate::content::models::{ContentKind, ContentSource, UnifiedContentId};
use crate::storage::error::{StorageError, StorageResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDocument {
    pub unified_id: String,
    pub title: String,
    pub body: String,
    pub tags: String,
    pub aliases: String,
}

pub fn build_search_document(conn: &Connection, unified_id: &str) -> StorageResult<SearchDocument> {
    UnifiedContentId::parse(unified_id).map_err(StorageError::Validation)?;
    let catalog = conn
        .query_row(
            "SELECT source, source_id, kind
             FROM content_catalog WHERE unified_id = ?1",
            params![unified_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Validation(format!("content catalog row not found: {unified_id}"))
        })?;
    let (source, source_id, kind) = catalog;
    let source = ContentSource::parse(&source).ok_or_else(|| {
        StorageError::Validation(format!("unknown content source for {unified_id}: {source}"))
    })?;
    if UnifiedContentId::new(source, &source_id)
        .map_err(StorageError::Validation)?
        .as_str()
        != unified_id
    {
        return Err(StorageError::Validation(format!(
            "content catalog identity mismatch for {unified_id}"
        )));
    }
    let kind = parse_kind(&kind, unified_id)?;

    match source {
        ContentSource::Dock => build_dock_document(conn, unified_id, &source_id, kind),
        ContentSource::Vault => build_vault_document(conn, unified_id, &source_id, kind),
    }
}

pub fn replace_projection(conn: &Connection, document: &SearchDocument) -> StorageResult<()> {
    conn.execute_batch("SAVEPOINT content_projection_replace")?;
    let replacement = (|| {
        conn.execute(
            "DELETE FROM content_fts WHERE unified_id = ?1",
            params![&document.unified_id],
        )?;
        conn.execute(
            "INSERT INTO content_fts(unified_id, title, body, tags, aliases)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                &document.unified_id,
                &document.title,
                &document.body,
                &document.tags,
                &document.aliases,
            ],
        )?;
        Ok(())
    })();

    match replacement {
        Ok(()) => {
            conn.execute_batch("RELEASE SAVEPOINT content_projection_replace")?;
            Ok(())
        }
        Err(error) => {
            conn.execute_batch(
                "ROLLBACK TO SAVEPOINT content_projection_replace;
                 RELEASE SAVEPOINT content_projection_replace;",
            )?;
            Err(error)
        }
    }
}

pub fn delete_projection(conn: &Connection, unified_id: &str) -> StorageResult<()> {
    UnifiedContentId::parse(unified_id).map_err(StorageError::Validation)?;
    conn.execute(
        "DELETE FROM content_fts WHERE unified_id = ?1",
        params![unified_id],
    )?;
    Ok(())
}

pub(crate) fn backfill_missing_projections(conn: &Connection) -> StorageResult<()> {
    let ids = {
        let mut stmt = conn.prepare(
            "SELECT c.unified_id
             FROM content_catalog c
             WHERE NOT EXISTS (
                 SELECT 1 FROM content_fts f WHERE f.unified_id = c.unified_id
             )
             ORDER BY c.unified_id ASC",
        )?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids
    };
    for id in ids {
        let document = build_search_document(conn, &id)?;
        replace_projection(conn, &document)?;
    }
    Ok(())
}

fn parse_kind(value: &str, unified_id: &str) -> StorageResult<ContentKind> {
    let kind = match value {
        "text" => ContentKind::Text,
        "image" => ContentKind::Image,
        "file" => ContentKind::File,
        "credential" => ContentKind::Credential,
        "bookmark" => ContentKind::Bookmark,
        "note" => ContentKind::Note,
        _ => {
            return Err(StorageError::Validation(format!(
                "unknown content kind for {unified_id}: {value}"
            )))
        }
    };
    Ok(kind)
}

fn build_dock_document(
    conn: &Connection,
    unified_id: &str,
    source_id: &str,
    catalog_kind: ContentKind,
) -> StorageResult<SearchDocument> {
    if !matches!(
        catalog_kind,
        ContentKind::Text | ContentKind::Image | ContentKind::File
    ) {
        return Err(source_kind_mismatch(unified_id));
    }
    let payload = conn
        .query_row(
            "SELECT kind, content, file_name, title FROM entries WHERE id = ?1",
            params![source_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Validation(format!("Dock payload not found for {unified_id}"))
        })?;
    let (payload_kind, content, file_name, persisted_title) = payload;
    if parse_kind(&payload_kind, unified_id)? != catalog_kind {
        return Err(source_kind_mismatch(unified_id));
    }

    let persisted_title = persisted_title
        .as_deref()
        .map(normalize_text)
        .unwrap_or_default();
    let file_name = file_name.as_deref().map(normalize_text).unwrap_or_default();
    let content_body = content.as_deref().map(normalize_text).unwrap_or_default();
    let title = if !persisted_title.is_empty() {
        persisted_title
    } else {
        match catalog_kind {
            ContentKind::Text => content
                .as_deref()
                .and_then(first_non_empty_line)
                .unwrap_or_default(),
            ContentKind::Image | ContentKind::File => file_name.clone(),
            _ => return Err(source_kind_mismatch(unified_id)),
        }
    };
    let body = join_unique([content_body, file_name]);

    Ok(SearchDocument {
        unified_id: unified_id.to_string(),
        title,
        body,
        tags: String::new(),
        aliases: String::new(),
    })
}

fn build_vault_document(
    conn: &Connection,
    unified_id: &str,
    source_id: &str,
    catalog_kind: ContentKind,
) -> StorageResult<SearchDocument> {
    if !matches!(
        catalog_kind,
        ContentKind::Credential | ContentKind::Bookmark | ContentKind::Note
    ) {
        return Err(source_kind_mismatch(unified_id));
    }
    let payload = conn
        .query_row(
            "SELECT kind, title, notes FROM vault_entries WHERE id = ?1",
            params![source_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Validation(format!("Vault payload not found for {unified_id}"))
        })?;
    let (payload_kind, title, notes) = payload;
    if parse_kind(&payload_kind, unified_id)? != catalog_kind {
        return Err(source_kind_mismatch(unified_id));
    }

    let fields = {
        let mut stmt = conn.prepare(
            "SELECT key, value
             FROM vault_fields
             WHERE entry_id = ?1 AND is_sensitive = 0
             ORDER BY sort_order ASC, key ASC, id ASC",
        )?;
        let rows = stmt
            .query_map(params![source_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let tags = {
        let mut stmt = conn.prepare(
            "SELECT tag
             FROM vault_tags
             WHERE entry_id = ?1
             ORDER BY normalized_tag ASC, source ASC, tag ASC",
        )?;
        let rows = stmt
            .query_map(params![source_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let ai_metadata = conn
        .query_row(
            "SELECT summary, search_aliases_json, status
             FROM vault_ai_metadata WHERE entry_id = ?1",
            params![source_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;

    let mut body_parts = Vec::with_capacity(fields.len() + 2);
    if let Some(notes) = notes {
        body_parts.push(normalize_text(&notes));
    }
    for (key, value) in fields {
        body_parts.push(join_unique([normalize_text(&key), normalize_text(&value)]));
    }
    let aliases = if let Some((summary, aliases_json, status)) = ai_metadata {
        if status == "ready" {
            if let Some(summary) = summary {
                body_parts.push(normalize_text(&summary));
            }
            let parsed: Vec<String> = serde_json::from_str(&aliases_json)?;
            join_unique(parsed.into_iter().map(|alias| normalize_text(&alias)))
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    Ok(SearchDocument {
        unified_id: unified_id.to_string(),
        title: normalize_text(&title),
        body: join_unique(body_parts),
        tags: join_unique(tags.into_iter().map(|tag| normalize_text(&tag))),
        aliases,
    })
}

fn source_kind_mismatch(unified_id: &str) -> StorageError {
    StorageError::Validation(format!(
        "content source and kind do not match for {unified_id}"
    ))
}

pub(crate) fn normalize_text(value: &str) -> String {
    let mut normalized = String::new();
    let mut needs_space = false;
    for character in value.chars() {
        if character.is_whitespace() {
            needs_space = !normalized.is_empty();
        } else if character.is_control() {
            continue;
        } else {
            if needs_space {
                normalized.push(' ');
                needs_space = false;
            }
            normalized.push(character);
        }
    }
    normalized
}

fn first_non_empty_line(value: &str) -> Option<String> {
    value.lines().find_map(|line| {
        let normalized = normalize_text(line);
        (!normalized.is_empty()).then(|| normalized.chars().take(80).collect())
    })
}

fn join_unique(parts: impl IntoIterator<Item = String>) -> String {
    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    for part in parts {
        if !part.is_empty() && seen.insert(part.clone()) {
            kept.push(part);
        }
    }
    kept.join(" ")
}

#[cfg(test)]
pub(crate) mod tests {
    use rusqlite::{params, Connection};

    use super::{build_search_document, delete_projection, replace_projection};
    use crate::content::catalog::catalog_ids_for_scope;
    use crate::content::migrations::ensure_content_schema;
    use crate::content::models::BrowseScope;
    use crate::scratchpad::storage::ensure_dock_schema;
    use crate::storage::error::StorageError;
    use crate::vault::storage::ensure_vault_schema;

    pub(crate) const SENSITIVE_LITERAL: &str = "NeverIndexMe";
    pub(crate) const FILE_PATH_LITERAL: &str = "PrivateAssetPath";

    pub(crate) fn fixture_with_all_kinds() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        ensure_dock_schema(&mut conn).unwrap();
        ensure_vault_schema(&mut conn).unwrap();

        let dock_rows = [
            (
                "text-1",
                "text",
                Some("  数据库维护窗口\n周六执行\u{0007}  "),
                None,
                None,
                None,
                "2026-07-10T08:00:00+00:00",
                "2026-07-18T08:00:00+00:00",
            ),
            (
                "image-1",
                "image",
                None,
                Some("C:/PrivateAssetPath/architecture.png"),
                Some("架构图.png"),
                Some("系统架构图"),
                "2026-07-11T08:00:00+00:00",
                "2026-07-17T08:00:00+00:00",
            ),
            (
                "file-1",
                "file",
                None,
                Some("C:/PrivateAssetPath/release.pdf"),
                Some("上线清单.pdf"),
                None,
                "2026-07-12T08:00:00+00:00",
                "2026-07-16T08:00:00+00:00",
            ),
        ];
        for (id, kind, content, file_path, file_name, title, created_at, updated_at) in dock_rows {
            conn.execute(
                "INSERT INTO entries(
                    id, kind, content, file_path, file_name, title, source,
                    created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'fixture', ?7, ?8)",
                params![id, kind, content, file_path, file_name, title, created_at, updated_at],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO home_entries(entry_id, created_at, sort_order) VALUES
             ('text-1', '2026-07-18T08:00:00+00:00', 0.0),
             ('image-1', '2026-07-17T08:00:00+00:00', 1.0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO note_entries(entry_id, created_at, sort_order)
             VALUES ('file-1', '2026-07-16T08:00:00+00:00', 0.0)",
            [],
        )
        .unwrap();

        let vault_rows = [
            (
                "credential-1",
                "credential",
                "Production login",
                Some("Rotate monthly"),
                "2026-07-12T09:00:00+00:00",
                "2026-07-15T08:00:00+00:00",
            ),
            (
                "bookmark-1",
                "bookmark",
                "Operations console",
                Some("Primary admin portal"),
                "2026-07-13T08:00:00+00:00",
                "2026-07-14T08:00:00+00:00",
            ),
            (
                "note-1",
                "note",
                "Release note",
                Some("Remember the rollback window"),
                "2026-07-14T08:00:00+00:00",
                "2026-07-13T08:00:00+00:00",
            ),
        ];
        for (id, kind, title, notes, created_at, updated_at) in vault_rows {
            conn.execute(
                "INSERT INTO vault_entries(id, kind, title, notes, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, kind, title, notes, created_at, updated_at],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO vault_fields(
                id, entry_id, key, value, is_sensitive, sort_order
             ) VALUES
             ('field-user', 'credential-1', 'username', 'alice', 0, 0),
             ('field-password', 'credential-1', 'NeverIndexMeKey', 'NeverIndexMe', 1, 1),
             ('field-url', 'bookmark-1', 'url', 'https://console.example.test', 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vault_tags(entry_id, tag, normalized_tag, source) VALUES
             ('bookmark-1', '紧急', '紧急', 'manual'),
             ('bookmark-1', '生产环境', '生产环境', 'manual'),
             ('credential-1', 'Access', 'access', 'manual')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vault_ai_metadata(
                entry_id, summary, search_aliases_json, content_hash, status
             ) VALUES
             ('bookmark-1', 'Approved safe summary',
              '[\"prod console\",\"prod console\",\" production \",\"\"]',
              'hash-ready', 'ready'),
             ('credential-1', 'NeverPendingSummary',
              '[\"NeverPendingAlias\"]', 'hash-pending', 'pending'),
             ('note-1', 'NeverErrorSummary',
              '[\"NeverErrorAlias\"]', 'hash-error', 'error')",
            [],
        )
        .unwrap();

        ensure_content_schema(&mut conn, 7).unwrap();
        refresh_all_projections(&conn);
        conn
    }

    pub(crate) fn refresh_all_projections(conn: &Connection) {
        for id in catalog_ids_for_scope(conn, BrowseScope::All).unwrap() {
            let document = build_search_document(conn, &id).unwrap();
            replace_projection(conn, &document).unwrap();
        }
    }

    #[test]
    fn projection_indexes_useful_fields_but_excludes_sensitive_and_unapproved_values() {
        let conn = fixture_with_all_kinds();
        let credential = build_search_document(&conn, "vault:credential-1").unwrap();
        let bookmark = build_search_document(&conn, "vault:bookmark-1").unwrap();
        let note = build_search_document(&conn, "vault:note-1").unwrap();
        let file = build_search_document(&conn, "dock:file-1").unwrap();

        assert!(credential.body.contains("username alice"));
        assert_eq!(credential.tags, "Access");
        assert!(!format!(
            "{} {} {} {}",
            credential.title, credential.body, credential.tags, credential.aliases
        )
        .contains(SENSITIVE_LITERAL));
        assert!(!credential.body.contains("NeverPendingSummary"));
        assert!(!credential.aliases.contains("NeverPendingAlias"));
        assert!(bookmark.body.contains("Approved safe summary"));
        assert_eq!(bookmark.tags, "生产环境 紧急");
        assert_eq!(bookmark.aliases, "prod console production");
        assert!(!note.body.contains("NeverErrorSummary"));
        assert!(!note.aliases.contains("NeverErrorAlias"));
        assert_eq!(file.title, "上线清单.pdf");
        assert!(file.body.contains("上线清单.pdf"));
        assert!(!file.body.contains(FILE_PATH_LITERAL));

        let projected: Vec<(String, String, String, String)> = conn
            .prepare("SELECT title, body, tags, aliases FROM content_fts ORDER BY unified_id")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let visible = format!("{projected:?}");
        assert!(!visible.contains(SENSITIVE_LITERAL));
        assert!(!visible.contains(FILE_PATH_LITERAL));
    }

    #[test]
    fn dock_text_fallback_title_is_trimmed_and_truncated_by_unicode_character() {
        let mut conn = fixture_with_all_kinds();
        let long_line = "界".repeat(90);
        conn.execute(
            "INSERT INTO entries(
                id, kind, content, source, created_at, updated_at
             ) VALUES ('unicode-title', 'text', ?1, 'fixture',
                       '2026-07-15T08:00:00+00:00', '2026-07-15T08:00:00+00:00')",
            params![format!("\n  \n{long_line}\u{0000}\n{}", "汉".repeat(200))],
        )
        .unwrap();
        ensure_content_schema(&mut conn, 7).unwrap();

        let document = build_search_document(&conn, "dock:unicode-title").unwrap();

        assert_eq!(document.title, "界".repeat(80));
        assert_eq!(document.title.chars().count(), 80);
        assert!(!document.body.chars().any(char::is_control));
    }

    #[test]
    fn projection_rejects_invalid_ids_missing_payloads_kind_mismatches_and_bad_alias_json() {
        let conn = fixture_with_all_kinds();
        let invalid = build_search_document(&conn, "not-a-unified-id").unwrap_err();
        assert!(matches!(invalid, StorageError::Validation(_)));

        let missing = build_search_document(&conn, "dock:missing").unwrap_err();
        assert!(matches!(missing, StorageError::Validation(_)));

        conn.execute("DELETE FROM entries WHERE id = 'image-1'", [])
            .unwrap();
        let missing_payload = build_search_document(&conn, "dock:image-1").unwrap_err();
        assert!(matches!(missing_payload, StorageError::Validation(_)));

        conn.execute(
            "UPDATE content_catalog SET kind = 'credential' WHERE unified_id = 'dock:text-1'",
            [],
        )
        .unwrap();
        let mismatched = build_search_document(&conn, "dock:text-1").unwrap_err();
        assert!(matches!(mismatched, StorageError::Validation(_)));

        conn.execute(
            "UPDATE vault_ai_metadata SET search_aliases_json = '{bad json'
             WHERE entry_id = 'bookmark-1'",
            [],
        )
        .unwrap();
        assert!(matches!(
            build_search_document(&conn, "vault:bookmark-1").unwrap_err(),
            StorageError::Serialization(_)
        ));
    }

    #[test]
    fn replace_and_delete_projection_keep_exactly_one_row_per_unified_id() {
        let conn = fixture_with_all_kinds();
        let mut document = build_search_document(&conn, "dock:text-1").unwrap();
        document.title = "Replacement".to_string();

        replace_projection(&conn, &document).unwrap();
        replace_projection(&conn, &document).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM content_fts WHERE unified_id = 'dock:text-1'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT title FROM content_fts WHERE unified_id = 'dock:text-1'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "Replacement"
        );

        delete_projection(&conn, "dock:text-1").unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM content_fts WHERE unified_id = 'dock:text-1'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn failed_projection_replacement_restores_the_previous_row() {
        let conn = fixture_with_all_kinds();
        let old_projection: (String, String, String, String) = conn
            .query_row(
                "SELECT title, body, tags, aliases
                 FROM content_fts WHERE unified_id = 'dock:text-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        conn.execute_batch(
            r#"
            DROP TABLE content_fts;
            CREATE TABLE content_fts (
                unified_id TEXT NOT NULL,
                title TEXT NOT NULL CHECK (title != 'Rejected replacement'),
                body TEXT NOT NULL,
                tags TEXT NOT NULL,
                aliases TEXT NOT NULL
            );
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO content_fts(unified_id, title, body, tags, aliases)
             VALUES ('dock:text-1', ?1, ?2, ?3, ?4)",
            params![
                &old_projection.0,
                &old_projection.1,
                &old_projection.2,
                &old_projection.3,
            ],
        )
        .unwrap();
        let mut replacement = build_search_document(&conn, "dock:text-1").unwrap();
        replacement.title = "Rejected replacement".to_string();

        assert!(replace_projection(&conn, &replacement).is_err());

        assert_eq!(
            conn.query_row(
                "SELECT title, body, tags, aliases
                 FROM content_fts WHERE unified_id = 'dock:text-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap(),
            old_projection
        );
    }
}
