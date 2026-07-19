use std::collections::BTreeSet;
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::content::catalog::{
    bump_revision, catalog_entry_by_id, current_revision, parse_kind, parse_timestamp,
    summaries_for_scope, summary_by_id, top_position, CatalogEntry,
};
use crate::content::models::{
    BrowseScope, ContentChange, ContentDetail, ContentKind, ContentMutation, ContentOperation,
    ContentSearchHit, ContentSource, ContentTagSource, UnifiedField, UnifiedQueryPlan, UnifiedTag,
};
use crate::storage::error::{StorageError, StorageResult};

pub fn list(
    conn: &Connection,
    scope: BrowseScope,
    kind: Option<ContentKind>,
) -> StorageResult<Vec<crate::content::models::ContentSummary>> {
    let reorderable = !matches!(scope, BrowseScope::All);
    Ok(summaries_for_scope(conn, scope, reorderable)?
        .into_iter()
        .filter(|summary| kind.is_none_or(|expected| summary.kind == expected))
        .collect())
}

pub fn detail(conn: &Connection, id: &str) -> StorageResult<ContentDetail> {
    let identity = catalog_entry_by_id(conn, id)?;
    let summary = summary_by_id(conn, id, false)?;

    match identity.source {
        ContentSource::Dock => dock_detail(conn, identity, summary),
        ContentSource::Vault => vault_detail(conn, identity, summary),
    }
}

pub fn search(
    conn: &Connection,
    query: &str,
    plan: Option<&UnifiedQueryPlan>,
    limit: usize,
) -> StorageResult<Vec<ContentSearchHit>> {
    crate::content::search::search_local(conn, query, plan, limit)
}

pub fn save(
    conn: &mut Connection,
    id: &str,
) -> StorageResult<ContentMutation<crate::content::models::ContentSummary>> {
    save_at(conn, id, Utc::now())
}

pub fn unsave(
    conn: &mut Connection,
    id: &str,
    cleanup_days: i64,
) -> StorageResult<ContentMutation<crate::content::models::ContentSummary>> {
    unsave_at(conn, id, cleanup_days, Utc::now())
}

pub fn reorder(
    conn: &mut Connection,
    scope: BrowseScope,
    ordered_ids: &[String],
) -> StorageResult<ContentMutation<()>> {
    let (retention, position_column, membership_table) = match scope {
        BrowseScope::Temporary => (
            crate::content::models::RetentionState::Temporary,
            "inbox_position",
            "home_entries",
        ),
        BrowseScope::Saved => (
            crate::content::models::RetentionState::Saved,
            "saved_position",
            "note_entries",
        ),
        BrowseScope::All => {
            return Err(StorageError::Validation(
                "all-content scope cannot be reordered".to_string(),
            ))
        }
    };
    let supplied = ordered_ids.iter().collect::<BTreeSet<_>>();
    if supplied.len() != ordered_ids.len() {
        return Err(StorageError::Validation(
            "reorder IDs must not contain duplicates".to_string(),
        ));
    }

    let tx = conn.transaction()?;
    ensure_not_pending(&tx, ordered_ids)?;
    let current_ids = crate::content::catalog::catalog_ids_for_scope(&tx, scope)?;
    let current = current_ids.iter().collect::<BTreeSet<_>>();
    if supplied != current {
        return Err(StorageError::Validation(
            "reorder IDs must exactly match the current scope".to_string(),
        ));
    }
    if ordered_ids.is_empty() {
        let revision = crate::content::catalog::current_revision(&tx)?;
        tx.commit()?;
        return Ok(ContentMutation {
            value: (),
            revision,
            changes: Vec::new(),
        });
    }

    let state = match retention {
        crate::content::models::RetentionState::Temporary => "temporary",
        crate::content::models::RetentionState::Saved => "saved",
    };
    for (index, id) in ordered_ids.iter().enumerate() {
        let identity = catalog_entry_by_id(&tx, id)?;
        let position = index as f64;
        let affected = tx.execute(
            &format!(
                "UPDATE content_catalog SET {position_column}=?2
                 WHERE unified_id=?1 AND retention_state=?3"
            ),
            params![id, position, state],
        )?;
        if affected != 1 {
            return Err(StorageError::Validation(format!(
                "content left the reorder scope: {id}"
            )));
        }
        if identity.source == ContentSource::Dock {
            let affected = tx.execute(
                &format!("UPDATE {membership_table} SET sort_order=?2 WHERE entry_id=?1"),
                params![identity.source_id, position],
            )?;
            if affected != 1 {
                return Err(StorageError::Validation(format!(
                    "Dock membership missing while reordering {id}"
                )));
            }
        }
    }
    let revision = bump_revision(&tx)?;
    tx.commit()?;

    Ok(ContentMutation {
        value: (),
        revision,
        changes: ordered_ids
            .iter()
            .map(|id| ContentChange {
                id: id.clone(),
                operation: ContentOperation::Reordered,
            })
            .collect(),
    })
}

pub fn delete(conn: &mut Connection, id: &str) -> StorageResult<ContentMutation<()>> {
    let tx = conn.transaction()?;
    ensure_no_pending_delete(&tx, id)?;
    let attachment = delete_in_transaction(&tx, id)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    remove_attachment(id, attachment);

    Ok(ContentMutation {
        value: (),
        revision,
        changes: vec![ContentChange {
            id: id.to_string(),
            operation: ContentOperation::Deleted,
        }],
    })
}

pub fn delete_temporary(conn: &mut Connection, id: &str) -> StorageResult<ContentMutation<()>> {
    let tx = conn.transaction()?;
    ensure_not_pending(&tx, &[id.to_string()])?;
    let identity = catalog_entry_by_id(&tx, id)?;
    if identity.retention != crate::content::models::RetentionState::Temporary {
        return Err(StorageError::Validation(format!(
            "content is no longer temporary: {id}"
        )));
    }
    ensure_payload_identity(&tx, id, &identity)?;
    if identity.source == ContentSource::Dock {
        let memberships: (i64, i64) = tx.query_row(
            "SELECT
                 EXISTS(SELECT 1 FROM home_entries WHERE entry_id=?1),
                 EXISTS(SELECT 1 FROM note_entries WHERE entry_id=?1)",
            params![identity.source_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if memberships != (1, 0) {
            return Err(StorageError::Validation(format!(
                "temporary Dock content has inconsistent memberships: {id}"
            )));
        }
    }
    let attachment = delete_in_transaction(&tx, id)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    remove_attachment(id, attachment);

    Ok(ContentMutation {
        value: (),
        revision,
        changes: vec![ContentChange {
            id: id.to_string(),
            operation: ContentOperation::Deleted,
        }],
    })
}

pub fn cleanup_expired(
    conn: &mut Connection,
    now: DateTime<Utc>,
) -> StorageResult<ContentMutation<usize>> {
    let tx = conn.transaction()?;
    let candidate_ids = {
        let mut stmt = tx.prepare(
            "SELECT c.unified_id
             FROM content_catalog c
             WHERE c.retention_state='temporary'
               AND c.cleanup_at IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1 FROM content_pending_deletes p
                   WHERE p.unified_id=c.unified_id
               )
             ORDER BY c.unified_id ASC",
        )?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids
    };
    let mut due_ids = Vec::new();
    for id in candidate_ids {
        let catalog = catalog_entry_by_id(&tx, &id)?;
        let cleanup_at = catalog.cleanup_at.as_deref().ok_or_else(|| {
            StorageError::Validation(format!("cleanup timestamp disappeared for {id}"))
        })?;
        if parse_timestamp(cleanup_at, "cleanup_at", &id)? <= now {
            due_ids.push(id);
        }
    }
    if due_ids.is_empty() {
        let revision = current_revision(&tx)?;
        tx.commit()?;
        return Ok(ContentMutation {
            value: 0,
            revision,
            changes: Vec::new(),
        });
    }

    let mut attachments = Vec::new();
    for id in &due_ids {
        attachments.push((id.clone(), delete_in_transaction(&tx, id)?));
    }
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    for (id, attachment) in attachments {
        remove_attachment(&id, attachment);
    }

    Ok(ContentMutation {
        value: due_ids.len(),
        revision,
        changes: due_ids
            .into_iter()
            .map(|id| ContentChange {
                id,
                operation: ContentOperation::Deleted,
            })
            .collect(),
    })
}

pub(crate) fn delete_in_transaction(conn: &Connection, id: &str) -> StorageResult<Option<String>> {
    let identity = catalog_entry_by_id(conn, id)?;
    ensure_payload_identity(conn, id, &identity)?;
    let attachment = if identity.source == ContentSource::Dock
        && matches!(identity.kind, ContentKind::Image | ContentKind::File)
    {
        conn.query_row(
            "SELECT file_path FROM entries WHERE id=?1",
            params![identity.source_id],
            |row| row.get::<_, Option<String>>(0),
        )?
    } else {
        None
    };

    match identity.source {
        ContentSource::Dock => {
            conn.execute(
                "DELETE FROM home_entries WHERE entry_id=?1",
                params![identity.source_id],
            )?;
            conn.execute(
                "DELETE FROM note_entries WHERE entry_id=?1",
                params![identity.source_id],
            )?;
            let affected = conn.execute(
                "DELETE FROM entries WHERE id=?1",
                params![identity.source_id],
            )?;
            if affected != 1 {
                return Err(StorageError::Validation(format!(
                    "Dock payload disappeared for {id}"
                )));
            }
        }
        ContentSource::Vault => {
            conn.execute(
                "DELETE FROM vault_capture_requests WHERE entry_id=?1",
                params![identity.source_id],
            )?;
            conn.execute(
                "DELETE FROM vault_ai_metadata WHERE entry_id=?1",
                params![identity.source_id],
            )?;
            conn.execute(
                "DELETE FROM vault_tags WHERE entry_id=?1",
                params![identity.source_id],
            )?;
            conn.execute(
                "DELETE FROM vault_fields WHERE entry_id=?1",
                params![identity.source_id],
            )?;
            conn.execute(
                "DELETE FROM vault_fts WHERE entry_id=?1",
                params![identity.source_id],
            )?;
            let affected = conn.execute(
                "DELETE FROM vault_entries WHERE id=?1",
                params![identity.source_id],
            )?;
            if affected != 1 {
                return Err(StorageError::Validation(format!(
                    "Vault payload disappeared for {id}"
                )));
            }
        }
    }
    conn.execute(
        "DELETE FROM content_pending_deletes WHERE unified_id=?1",
        params![id],
    )?;
    conn.execute("DELETE FROM content_fts WHERE unified_id=?1", params![id])?;
    let affected = conn.execute(
        "DELETE FROM content_catalog WHERE unified_id=?1",
        params![id],
    )?;
    if affected != 1 {
        return Err(StorageError::Validation(format!(
            "content catalog row disappeared: {id}"
        )));
    }
    Ok(attachment)
}

pub(crate) fn remove_attachment(id: &str, attachment: Option<String>) {
    if let Some(attachment) = attachment {
        if let Err(error) = std::fs::remove_file(attachment) {
            eprintln!("failed to remove attachment for {id}: {error}");
        }
    }
}

fn save_at(
    conn: &mut Connection,
    id: &str,
    now: DateTime<Utc>,
) -> StorageResult<ContentMutation<crate::content::models::ContentSummary>> {
    let tx = conn.transaction()?;
    ensure_not_pending(&tx, &[id.to_string()])?;
    let identity = catalog_entry_by_id(&tx, id)?;
    if identity.retention != crate::content::models::RetentionState::Temporary {
        return Err(StorageError::Validation(format!(
            "content is not temporary: {id}"
        )));
    }
    ensure_payload_identity(&tx, id, &identity)?;
    let position = top_position(&tx, crate::content::models::RetentionState::Saved)?;
    let transition = now.to_rfc3339();
    let affected = tx.execute(
        "UPDATE content_catalog
         SET retention_state='saved', retention_changed_at=?2, cleanup_at=NULL,
             saved_position=?3
         WHERE unified_id=?1 AND retention_state='temporary'",
        params![id, transition, position],
    )?;
    if affected != 1 {
        return Err(StorageError::Validation(format!(
            "content retention changed concurrently: {id}"
        )));
    }
    if identity.source == ContentSource::Dock {
        tx.execute(
            "INSERT INTO note_entries(entry_id, created_at, sort_order)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(entry_id) DO UPDATE SET
                 created_at=excluded.created_at, sort_order=excluded.sort_order",
            params![identity.source_id, transition, position],
        )?;
    }
    let summary = summary_by_id(&tx, id, true)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;

    Ok(retention_mutation(summary, revision, id))
}

fn unsave_at(
    conn: &mut Connection,
    id: &str,
    cleanup_days: i64,
    now: DateTime<Utc>,
) -> StorageResult<ContentMutation<crate::content::models::ContentSummary>> {
    let cleanup_delta = crate::content::migrations::validate_cleanup_days(cleanup_days)?;
    let transition = now.to_rfc3339();
    let cleanup_at = if cleanup_delta.is_zero() {
        transition.clone()
    } else {
        now.checked_add_signed(cleanup_delta)
            .ok_or_else(|| {
                StorageError::Validation(format!("cleanup timestamp is out of range for {id}"))
            })?
            .to_rfc3339()
    };
    let tx = conn.transaction()?;
    ensure_not_pending(&tx, &[id.to_string()])?;
    let identity = catalog_entry_by_id(&tx, id)?;
    if identity.retention != crate::content::models::RetentionState::Saved {
        return Err(StorageError::Validation(format!(
            "content is not saved: {id}"
        )));
    }
    ensure_payload_identity(&tx, id, &identity)?;
    let position = top_position(&tx, crate::content::models::RetentionState::Temporary)?;
    let affected = tx.execute(
        "UPDATE content_catalog
         SET retention_state='temporary', retention_changed_at=?2, cleanup_at=?3,
             inbox_position=?4, saved_position=NULL
         WHERE unified_id=?1 AND retention_state='saved'",
        params![id, transition, cleanup_at, position],
    )?;
    if affected != 1 {
        return Err(StorageError::Validation(format!(
            "content retention changed concurrently: {id}"
        )));
    }
    if identity.source == ContentSource::Dock {
        tx.execute(
            "DELETE FROM note_entries WHERE entry_id=?1",
            params![identity.source_id],
        )?;
        tx.execute(
            "INSERT INTO home_entries(entry_id, created_at, sort_order)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(entry_id) DO UPDATE SET sort_order=excluded.sort_order",
            params![identity.source_id, transition, position],
        )?;
    }
    let summary = summary_by_id(&tx, id, true)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;

    Ok(retention_mutation(summary, revision, id))
}

fn retention_mutation(
    value: crate::content::models::ContentSummary,
    revision: i64,
    id: &str,
) -> ContentMutation<crate::content::models::ContentSummary> {
    ContentMutation {
        value,
        revision,
        changes: vec![ContentChange {
            id: id.to_string(),
            operation: ContentOperation::Retention,
        }],
    }
}

fn ensure_not_pending(conn: &Connection, ids: &[String]) -> StorageResult<()> {
    for id in ids {
        ensure_no_pending_delete(conn, id)?;
    }
    Ok(())
}

pub(crate) fn ensure_no_pending_delete(conn: &Connection, id: &str) -> StorageResult<()> {
    if conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM content_pending_deletes WHERE unified_id=?1)",
        params![id],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(StorageError::Validation(format!(
            "content has a pending delete: {id}"
        )));
    }
    Ok(())
}

fn ensure_payload_identity(
    conn: &Connection,
    id: &str,
    identity: &CatalogEntry,
) -> StorageResult<()> {
    let (table, allowed) = match identity.source {
        ContentSource::Dock => (
            "entries",
            matches!(
                identity.kind,
                ContentKind::Text | ContentKind::Image | ContentKind::File
            ),
        ),
        ContentSource::Vault => (
            "vault_entries",
            matches!(
                identity.kind,
                ContentKind::Credential | ContentKind::Bookmark | ContentKind::Note
            ),
        ),
    };
    if !allowed {
        return Err(source_kind_mismatch(id));
    }
    let payload_kind = conn
        .query_row(
            &format!("SELECT kind FROM {table} WHERE id=?1"),
            params![identity.source_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| StorageError::Validation(format!("content payload not found for {id}")))?;
    if parse_kind(&payload_kind, id)? != identity.kind {
        return Err(source_kind_mismatch(id));
    }
    Ok(())
}

fn dock_detail(
    conn: &Connection,
    identity: CatalogEntry,
    summary: crate::content::models::ContentSummary,
) -> StorageResult<ContentDetail> {
    let payload = conn
        .query_row(
            "SELECT kind, content, file_path, file_name, mime_type, width, height,
                    size_bytes, title
             FROM entries WHERE id = ?1",
            params![identity.source_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Validation(format!("Dock payload not found for {}", summary.id))
        })?;
    let (kind, content, asset_path, file_name, mime_type, width, height, size_bytes, title) =
        payload;
    if parse_kind(&kind, &summary.id)? != identity.kind
        || !matches!(
            identity.kind,
            ContentKind::Text | ContentKind::Image | ContentKind::File
        )
    {
        return Err(source_kind_mismatch(&summary.id));
    }

    match identity.kind {
        ContentKind::Text => {
            let title = title
                .as_deref()
                .map(crate::content::projection::normalize_text)
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| summary.title.clone());
            Ok(ContentDetail::Text {
                title,
                body: content.unwrap_or_default(),
                summary,
            })
        }
        ContentKind::Image => {
            let asset_path = asset_path.unwrap_or_default();
            Ok(ContentDetail::Image {
                file_name: file_name.unwrap_or_default(),
                available: Path::new(&asset_path).is_file(),
                asset_path,
                mime_type,
                width,
                height,
                summary,
            })
        }
        ContentKind::File => {
            let asset_path = asset_path.unwrap_or_default();
            Ok(ContentDetail::File {
                file_name: file_name.unwrap_or_default(),
                available: Path::new(&asset_path).is_file(),
                asset_path,
                mime_type,
                size_bytes,
                summary,
            })
        }
        _ => Err(source_kind_mismatch(&summary.id)),
    }
}

fn vault_detail(
    conn: &Connection,
    identity: CatalogEntry,
    summary: crate::content::models::ContentSummary,
) -> StorageResult<ContentDetail> {
    let payload = conn
        .query_row(
            "SELECT kind, notes FROM vault_entries WHERE id = ?1",
            params![identity.source_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Validation(format!("Vault payload not found for {}", summary.id))
        })?;
    if parse_kind(&payload.0, &summary.id)? != identity.kind
        || !matches!(
            identity.kind,
            ContentKind::Credential | ContentKind::Bookmark | ContentKind::Note
        )
    {
        return Err(source_kind_mismatch(&summary.id));
    }
    let fields = vault_fields(conn, &identity.source_id)?;
    let tags = vault_tags(conn, &identity.source_id)?;

    match identity.kind {
        ContentKind::Credential => Ok(ContentDetail::Credential {
            summary,
            fields,
            notes: payload.1,
            tags,
        }),
        ContentKind::Bookmark => {
            let url = fields
                .iter()
                .find(|field| field.key.eq_ignore_ascii_case("url"))
                .map(|field| field.value.clone())
                .unwrap_or_default();
            Ok(ContentDetail::Bookmark {
                summary,
                url,
                fields,
                notes: payload.1,
                tags,
            })
        }
        ContentKind::Note => Ok(ContentDetail::Note {
            summary,
            body: payload.1.unwrap_or_default(),
            fields,
            tags,
        }),
        _ => Err(source_kind_mismatch(&summary.id)),
    }
}

fn vault_fields(conn: &Connection, source_id: &str) -> StorageResult<Vec<UnifiedField>> {
    let mut stmt = conn.prepare(
        "SELECT key, value, is_sensitive, sort_order
         FROM vault_fields WHERE entry_id = ?1
         ORDER BY sort_order ASC, key ASC, id ASC",
    )?;
    let fields = stmt
        .query_map(params![source_id], |row| {
            Ok(UnifiedField {
                key: row.get(0)?,
                value: row.get(1)?,
                is_sensitive: row.get::<_, i64>(2)? != 0,
                sort_order: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(fields)
}

fn vault_tags(conn: &Connection, source_id: &str) -> StorageResult<Vec<UnifiedTag>> {
    let mut stmt = conn.prepare(
        "SELECT tag, normalized_tag, source
         FROM vault_tags WHERE entry_id = ?1
         ORDER BY normalized_tag ASC, source ASC, tag ASC",
    )?;
    let tags = stmt
        .query_map(params![source_id], |row| {
            let source = row.get::<_, String>(2)?;
            let source = match source.as_str() {
                "manual" => ContentTagSource::Manual,
                "ai" => ContentTagSource::Ai,
                _ => {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("unknown Vault tag source: {source}"),
                        )),
                    ))
                }
            };
            Ok(UnifiedTag {
                tag: row.get(0)?,
                normalized_tag: row.get(1)?,
                source,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tags)
}

fn source_kind_mismatch(id: &str) -> StorageError {
    StorageError::Validation(format!("content source and kind do not match for {id}"))
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use rusqlite::types::Value;
    use rusqlite::{params, Connection};

    use super::{
        cleanup_expired, delete, delete_temporary, detail, list, reorder, save, search, unsave,
    };
    use crate::content::models::{
        BrowseScope, ContentDetail, ContentKind, RetentionState, UnifiedQueryPlan,
    };
    use crate::content::projection::tests::{
        fixture_with_all_kinds, refresh_all_projections, FILE_PATH_LITERAL,
    };
    use crate::models::entry::EntryView;
    use crate::storage::error::StorageError;

    fn revision(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT revision FROM content_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn scalar_i64(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |row| row.get(0)).unwrap()
    }

    fn rows_snapshot(conn: &Connection, sql: &str) -> Vec<Vec<Value>> {
        let mut statement = conn.prepare(sql).unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |row| {
                (0..columns)
                    .map(|column| row.get(column))
                    .collect::<rusqlite::Result<Vec<Value>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    #[test]
    fn failed_projection_write_rolls_back_payload_and_revision() {
        let mut conn = fixture_with_all_kinds();
        let start_revision = revision(&conn);
        conn.execute_batch(
            "CREATE TEMP TABLE content_fts_original AS SELECT * FROM content_fts;
             DROP TABLE content_fts;
             CREATE TABLE content_fts(
                 unified_id TEXT NOT NULL,
                 title TEXT NOT NULL,
                 body TEXT NOT NULL CHECK(body != 'must roll back'),
                 tags TEXT NOT NULL,
                 aliases TEXT NOT NULL
             );
             INSERT INTO content_fts SELECT * FROM content_fts_original;",
        )
        .unwrap();
        let snapshots = [
            "SELECT * FROM entries ORDER BY id",
            "SELECT * FROM home_entries ORDER BY entry_id",
            "SELECT * FROM note_entries ORDER BY entry_id",
            "SELECT * FROM vault_entries ORDER BY id",
            "SELECT * FROM vault_fields ORDER BY entry_id, sort_order, id",
            "SELECT * FROM vault_tags ORDER BY entry_id, normalized_tag, source",
            "SELECT * FROM vault_ai_metadata ORDER BY entry_id",
            "SELECT * FROM content_catalog ORDER BY unified_id",
            "SELECT * FROM content_fts ORDER BY unified_id",
            "SELECT * FROM vault_fts ORDER BY entry_id",
            "SELECT * FROM content_state ORDER BY singleton",
        ]
        .map(|sql| (sql, rows_snapshot(&conn, sql)));

        let error = crate::scratchpad::storage::create_text_entry_with_revision(
            &mut conn,
            EntryView::Home,
            "must roll back",
            "validation",
        )
        .unwrap_err();

        assert!(matches!(error, StorageError::Sqlite(_)));
        assert_eq!(revision(&conn), start_revision);
        for (sql, before) in snapshots {
            assert_eq!(rows_snapshot(&conn, sql), before, "changed snapshot: {sql}");
        }
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM entries WHERE content='must roll back'",
            ),
            0
        );
    }

    #[test]
    fn list_uses_scope_order_kind_filter_and_scope_reorder_capability() {
        let conn = fixture_with_all_kinds();

        let temporary = list(&conn, BrowseScope::Temporary, None).unwrap();
        assert_eq!(
            temporary
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["dock:text-1", "dock:image-1"]
        );
        assert!(temporary.iter().all(|item| item.capabilities.reorder));

        let saved_files = list(&conn, BrowseScope::Saved, Some(ContentKind::File)).unwrap();
        assert_eq!(saved_files.len(), 1);
        assert_eq!(saved_files[0].id, "dock:file-1");
        assert!(saved_files[0].capabilities.reorder);

        let all = list(&conn, BrowseScope::All, None).unwrap();
        assert_eq!(
            all.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
            vec![
                "dock:text-1",
                "dock:image-1",
                "dock:file-1",
                "vault:credential-1",
                "vault:bookmark-1",
                "vault:note-1",
            ]
        );
        assert!(all.iter().all(|item| !item.capabilities.reorder));
    }

    #[test]
    fn search_is_a_thin_wrapper_over_local_search() {
        let conn = fixture_with_all_kinds();
        let plan = UnifiedQueryPlan {
            kinds: vec![ContentKind::Bookmark],
            keywords: vec!["console".to_string()],
            ..UnifiedQueryPlan::default()
        };

        let wrapped = search(&conn, "operations", Some(&plan), 10).unwrap();
        let local =
            crate::content::search::search_local(&conn, "operations", Some(&plan), 10).unwrap();

        assert_eq!(wrapped, local);
    }

    #[test]
    fn detail_maps_all_six_payload_kinds_and_keeps_private_values_out_of_summary() {
        let conn = fixture_with_all_kinds();
        conn.execute(
            "UPDATE entries SET mime_type='image/png', width=640, height=480
             WHERE id='image-1'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE entries SET mime_type='application/pdf', size_bytes=4096
             WHERE id='file-1'",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vault_fields(id, entry_id, key, value, is_sensitive, sort_order)
             VALUES ('field-note', 'note-1', 'category', 'release', 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vault_tags(entry_id, tag, normalized_tag, source)
             VALUES ('credential-1', 'Suggested', 'suggested', 'ai')",
            [],
        )
        .unwrap();

        match detail(&conn, "dock:text-1").unwrap() {
            ContentDetail::Text {
                summary,
                title,
                body,
            } => {
                assert_eq!(title, summary.title);
                assert!(body.contains("数据库维护窗口"));
                assert!(!summary.capabilities.reorder);
            }
            other => panic!("unexpected detail: {other:?}"),
        }
        match detail(&conn, "dock:image-1").unwrap() {
            ContentDetail::Image {
                summary,
                file_name,
                asset_path,
                mime_type,
                width,
                height,
                available,
            } => {
                assert_eq!(file_name, "架构图.png");
                assert!(asset_path.contains(FILE_PATH_LITERAL));
                assert_eq!(mime_type.as_deref(), Some("image/png"));
                assert_eq!((width, height), (Some(640), Some(480)));
                assert!(!available);
                assert!(!serde_json::to_string(&summary)
                    .unwrap()
                    .contains(FILE_PATH_LITERAL));
            }
            other => panic!("unexpected detail: {other:?}"),
        }
        match detail(&conn, "dock:file-1").unwrap() {
            ContentDetail::File { size_bytes, .. } => assert_eq!(size_bytes, Some(4096)),
            other => panic!("unexpected detail: {other:?}"),
        }
        match detail(&conn, "vault:credential-1").unwrap() {
            ContentDetail::Credential {
                fields,
                notes,
                tags,
                ..
            } => {
                assert_eq!(notes.as_deref(), Some("Rotate monthly"));
                assert!(fields.iter().any(|field| {
                    field.value == "NeverIndexMe" && field.is_sensitive && field.sort_order == 1
                }));
                assert_eq!(
                    tags.iter().map(|tag| tag.source).collect::<Vec<_>>(),
                    vec![
                        crate::content::models::ContentTagSource::Manual,
                        crate::content::models::ContentTagSource::Ai,
                    ]
                );
            }
            other => panic!("unexpected detail: {other:?}"),
        }
        match detail(&conn, "vault:bookmark-1").unwrap() {
            ContentDetail::Bookmark { url, notes, .. } => {
                assert_eq!(url, "https://console.example.test");
                assert_eq!(notes.as_deref(), Some("Primary admin portal"));
            }
            other => panic!("unexpected detail: {other:?}"),
        }
        match detail(&conn, "vault:note-1").unwrap() {
            ContentDetail::Note { body, fields, .. } => {
                assert_eq!(body, "Remember the rollback window");
                assert_eq!(fields[0].key, "category");
            }
            other => panic!("unexpected detail: {other:?}"),
        }
    }

    #[test]
    fn detail_rejects_bad_opaque_ids_identity_kind_and_missing_payload() {
        let conn = fixture_with_all_kinds();
        assert!(matches!(
            detail(&conn, "text-1"),
            Err(StorageError::Validation(_))
        ));

        conn.execute(
            "UPDATE content_catalog SET source_id='mismatch' WHERE unified_id='dock:text-1'",
            [],
        )
        .unwrap();
        assert!(matches!(
            detail(&conn, "dock:text-1"),
            Err(StorageError::Validation(_))
        ));

        conn.execute("DELETE FROM entries WHERE id='file-1'", [])
            .unwrap();
        assert!(matches!(
            detail(&conn, "dock:file-1"),
            Err(StorageError::Validation(_))
        ));
    }

    #[test]
    fn text_detail_normalizes_persisted_title_and_falls_back_to_list_summary() {
        for (persisted, expected) in [
            (None, "数据库维护窗口"),
            (Some(""), "数据库维护窗口"),
            (Some("  \t\n  "), "数据库维护窗口"),
            (Some("\u{0007}\u{0000}"), "数据库维护窗口"),
            (Some("  Stored\u{0007}   title  "), "Stored title"),
        ] {
            let conn = fixture_with_all_kinds();
            conn.execute(
                "UPDATE entries SET title=?1 WHERE id='text-1'",
                params![persisted],
            )
            .unwrap();
            refresh_all_projections(&conn);

            let listed = list(&conn, BrowseScope::Temporary, Some(ContentKind::Text))
                .unwrap()
                .remove(0);
            match detail(&conn, "dock:text-1").unwrap() {
                ContentDetail::Text { summary, title, .. } => {
                    assert_eq!(listed.title, expected, "persisted={persisted:?}");
                    assert_eq!(title, listed.title, "persisted={persisted:?}");
                    assert_eq!(summary.title, listed.title, "persisted={persisted:?}");
                }
                other => panic!("unexpected detail: {other:?}"),
            }
        }
    }

    #[test]
    fn dock_and_vault_save_unsave_save_share_retention_and_revision_semantics() {
        let mut conn = fixture_with_all_kinds();
        let start = revision(&conn);

        let saved_dock = save(&mut conn, "dock:text-1").unwrap();
        assert_eq!(saved_dock.revision, start + 1);
        assert_eq!(saved_dock.value.retention, RetentionState::Saved);
        assert!(saved_dock.value.capabilities.reorder);
        assert_eq!(saved_dock.changes.len(), 1);
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM home_entries WHERE entry_id='text-1'"
            ),
            1
        );
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM note_entries WHERE entry_id='text-1'"
            ),
            1
        );
        let positions: (f64, f64) = conn
            .query_row(
                "SELECT c.saved_position, n.sort_order
                 FROM content_catalog c JOIN note_entries n ON n.entry_id=c.source_id
                 WHERE c.unified_id='dock:text-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(positions.0, positions.1);

        let temporary_dock = unsave(&mut conn, "dock:text-1", 7).unwrap();
        assert_eq!(temporary_dock.revision, start + 2);
        assert_eq!(temporary_dock.value.retention, RetentionState::Temporary);
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM note_entries WHERE entry_id='text-1'"
            ),
            0
        );
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM home_entries WHERE entry_id='text-1'"
            ),
            1
        );
        let changed = DateTime::parse_from_rfc3339(
            &conn.query_row(
                "SELECT retention_changed_at FROM content_catalog WHERE unified_id='dock:text-1'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        )
        .unwrap();
        let cleanup =
            DateTime::parse_from_rfc3339(temporary_dock.value.cleanup_at.as_deref().unwrap())
                .unwrap();
        assert_eq!(cleanup.signed_duration_since(changed).num_days(), 7);
        assert_eq!(save(&mut conn, "dock:text-1").unwrap().revision, start + 3);

        assert_eq!(
            unsave(&mut conn, "vault:credential-1", 0).unwrap().revision,
            start + 4
        );
        let vault_timestamps: (String, String) = conn
            .query_row(
                "SELECT retention_changed_at, cleanup_at FROM content_catalog
                 WHERE unified_id='vault:credential-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(vault_timestamps.0, vault_timestamps.1);
        assert_eq!(
            save(&mut conn, "vault:credential-1").unwrap().revision,
            start + 5
        );
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM vault_entries WHERE id='credential-1'"
            ),
            1
        );
    }

    #[test]
    fn invalid_transitions_and_cleanup_days_do_not_write_or_bump_revision() {
        let mut conn = fixture_with_all_kinds();
        let start = revision(&conn);

        assert!(matches!(
            save(&mut conn, "vault:credential-1"),
            Err(StorageError::Validation(_))
        ));
        assert!(matches!(
            unsave(&mut conn, "dock:text-1", 7),
            Err(StorageError::Validation(_))
        ));
        assert!(matches!(
            unsave(&mut conn, "vault:credential-1", -1),
            Err(StorageError::Validation(_))
        ));
        assert_eq!(revision(&conn), start);
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM note_entries WHERE entry_id='text-1'"
            ),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT retention_state FROM content_catalog WHERE unified_id='vault:credential-1'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "saved"
        );
    }

    #[test]
    fn save_projection_failure_rolls_back_retention_membership_position_and_revision() {
        let mut conn = fixture_with_all_kinds();
        let before: (String, Option<String>, Option<f64>, Option<f64>) = conn
            .query_row(
                "SELECT retention_state, cleanup_at, inbox_position, saved_position
                 FROM content_catalog WHERE unified_id='dock:text-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        let start = revision(&conn);
        conn.execute("DELETE FROM content_fts WHERE unified_id='dock:text-1'", [])
            .unwrap();

        assert!(matches!(
            save(&mut conn, "dock:text-1"),
            Err(StorageError::Validation(_))
        ));

        let after: (String, Option<String>, Option<f64>, Option<f64>) = conn
            .query_row(
                "SELECT retention_state, cleanup_at, inbox_position, saved_position
                 FROM content_catalog WHERE unified_id='dock:text-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(after, before);
        assert_eq!(revision(&conn), start);
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM home_entries WHERE entry_id='text-1'"
            ),
            1
        );
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM note_entries WHERE entry_id='text-1'"
            ),
            0
        );
    }

    #[test]
    fn unsave_projection_failure_rolls_back_retention_membership_position_and_revision() {
        let mut conn = fixture_with_all_kinds();
        let before: (String, Option<String>, Option<f64>, Option<f64>) = conn
            .query_row(
                "SELECT retention_state, cleanup_at, inbox_position, saved_position
                 FROM content_catalog WHERE unified_id='dock:file-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        let start = revision(&conn);
        conn.execute("DELETE FROM content_fts WHERE unified_id='dock:file-1'", [])
            .unwrap();

        assert!(matches!(
            unsave(&mut conn, "dock:file-1", 7),
            Err(StorageError::Validation(_))
        ));

        let after: (String, Option<String>, Option<f64>, Option<f64>) = conn
            .query_row(
                "SELECT retention_state, cleanup_at, inbox_position, saved_position
                 FROM content_catalog WHERE unified_id='dock:file-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(after, before);
        assert_eq!(revision(&conn), start);
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM home_entries WHERE entry_id='file-1'"
            ),
            0
        );
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM note_entries WHERE entry_id='file-1'"
            ),
            1
        );
    }

    #[test]
    fn save_rejects_unusable_top_positions_without_partial_transition() {
        for position in [None, Some(f64::INFINITY), Some(-9_007_199_254_740_992.0)] {
            let mut conn = fixture_with_all_kinds();
            conn.execute(
                "DELETE FROM content_catalog
                 WHERE retention_state='saved' AND unified_id<>'dock:file-1'",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE content_catalog SET saved_position=?1
                 WHERE unified_id='dock:file-1'",
                params![position],
            )
            .unwrap();
            let start = revision(&conn);

            assert!(matches!(
                save(&mut conn, "dock:text-1"),
                Err(StorageError::Validation(_))
            ));
            assert_eq!(revision(&conn), start);
            assert_eq!(
                conn.query_row(
                    "SELECT retention_state FROM content_catalog
                     WHERE unified_id='dock:text-1'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
                "temporary"
            );
            assert_eq!(
                scalar_i64(
                    &conn,
                    "SELECT COUNT(*) FROM note_entries WHERE entry_id='text-1'"
                ),
                0
            );
        }
    }

    #[test]
    fn reorder_requires_the_exact_scope_set_and_updates_catalog_and_dock_membership() {
        let mut conn = fixture_with_all_kinds();
        let start = revision(&conn);
        let ordered = vec!["dock:image-1".to_string(), "dock:text-1".to_string()];

        let mutation = reorder(&mut conn, BrowseScope::Temporary, &ordered).unwrap();

        assert_eq!(mutation.revision, start + 1);
        assert_eq!(
            mutation
                .changes
                .iter()
                .map(|change| change.id.as_str())
                .collect::<Vec<_>>(),
            vec!["dock:image-1", "dock:text-1"]
        );
        assert!(mutation
            .changes
            .iter()
            .all(|change| change.operation == crate::content::models::ContentOperation::Reordered));
        for (id, expected) in [("image-1", 0.0), ("text-1", 1.0)] {
            let positions: (f64, f64) = conn
                .query_row(
                    "SELECT c.inbox_position, h.sort_order
                     FROM content_catalog c JOIN home_entries h ON h.entry_id=c.source_id
                     WHERE c.source_id=?1",
                    params![id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(positions, (expected, expected));
        }

        let saved_ids = list(&conn, BrowseScope::Saved, None)
            .unwrap()
            .into_iter()
            .map(|summary| summary.id)
            .rev()
            .collect::<Vec<_>>();
        reorder(&mut conn, BrowseScope::Saved, &saved_ids).unwrap();
        let dock_file_index = saved_ids.iter().position(|id| id == "dock:file-1").unwrap() as f64;
        let saved_positions: (f64, f64) = conn
            .query_row(
                "SELECT c.saved_position, n.sort_order
                 FROM content_catalog c JOIN note_entries n ON n.entry_id=c.source_id
                 WHERE c.unified_id='dock:file-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(saved_positions, (dock_file_index, dock_file_index));
    }

    #[test]
    fn reorder_rejects_all_duplicates_missing_extra_and_cross_scope_without_writes() {
        let mut conn = fixture_with_all_kinds();
        let start = revision(&conn);
        let cases = [
            (BrowseScope::All, vec!["dock:text-1".to_string()]),
            (
                BrowseScope::Temporary,
                vec!["dock:text-1".to_string(), "dock:text-1".to_string()],
            ),
            (BrowseScope::Temporary, vec!["dock:text-1".to_string()]),
            (
                BrowseScope::Temporary,
                vec![
                    "dock:text-1".to_string(),
                    "dock:image-1".to_string(),
                    "dock:extra".to_string(),
                ],
            ),
            (
                BrowseScope::Temporary,
                vec!["dock:text-1".to_string(), "dock:file-1".to_string()],
            ),
        ];

        for (scope, ids) in cases {
            assert!(matches!(
                reorder(&mut conn, scope, &ids),
                Err(StorageError::Validation(_))
            ));
            assert_eq!(revision(&conn), start);
        }
        assert_eq!(
            list(&conn, BrowseScope::Temporary, None)
                .unwrap()
                .into_iter()
                .map(|summary| summary.id)
                .collect::<Vec<_>>(),
            vec!["dock:text-1", "dock:image-1"]
        );
    }

    #[test]
    fn reorder_rolls_back_catalog_memberships_and_revision_on_database_failure() {
        let mut conn = fixture_with_all_kinds();
        let start = revision(&conn);
        conn.execute_batch(
            "CREATE TRIGGER fail_home_reorder
             BEFORE UPDATE OF sort_order ON home_entries
             WHEN NEW.entry_id='image-1'
             BEGIN SELECT RAISE(ABORT, 'forced reorder failure'); END;",
        )
        .unwrap();

        let error = reorder(
            &mut conn,
            BrowseScope::Temporary,
            &["dock:image-1".to_string(), "dock:text-1".to_string()],
        )
        .unwrap_err();

        assert!(matches!(error, StorageError::Sqlite(_)));
        assert_eq!(revision(&conn), start);
        assert_eq!(
            conn.query_row(
                "SELECT inbox_position FROM content_catalog WHERE unified_id='dock:text-1'",
                [],
                |row| row.get::<_, f64>(0),
            )
            .unwrap(),
            0.0
        );
        assert_eq!(
            conn.query_row(
                "SELECT sort_order FROM home_entries WHERE entry_id='image-1'",
                [],
                |row| row.get::<_, f64>(0),
            )
            .unwrap(),
            1.0
        );
    }

    #[test]
    fn reorder_empty_scope_is_a_noop_without_revision_bump() {
        let mut conn = fixture_with_all_kinds();
        conn.execute(
            "UPDATE content_catalog SET retention_state='saved', inbox_position=NULL,
                 saved_position=100.0
             WHERE retention_state='temporary'",
            [],
        )
        .unwrap();
        let start = revision(&conn);

        let mutation = reorder(&mut conn, BrowseScope::Temporary, &[]).unwrap();

        assert_eq!(mutation.revision, start);
        assert!(mutation.changes.is_empty());
    }

    fn temp_asset(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "soma-content-service-{name}-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    #[test]
    fn delete_removes_dock_attachment_database_rows_projection_and_catalog() {
        let mut conn = fixture_with_all_kinds();
        let path = temp_asset("dock-delete");
        std::fs::write(&path, b"asset").unwrap();
        conn.execute(
            "UPDATE entries SET file_path=?1 WHERE id='image-1'",
            params![path.to_string_lossy()],
        )
        .unwrap();
        let start = revision(&conn);

        let mutation = delete(&mut conn, "dock:image-1").unwrap();

        assert_eq!(mutation.revision, start + 1);
        assert_eq!(mutation.changes.len(), 1);
        assert_eq!(
            mutation.changes[0].operation,
            crate::content::models::ContentOperation::Deleted
        );
        for sql in [
            "SELECT COUNT(*) FROM entries WHERE id='image-1'",
            "SELECT COUNT(*) FROM home_entries WHERE entry_id='image-1'",
            "SELECT COUNT(*) FROM note_entries WHERE entry_id='image-1'",
            "SELECT COUNT(*) FROM content_fts WHERE unified_id='dock:image-1'",
            "SELECT COUNT(*) FROM content_catalog WHERE unified_id='dock:image-1'",
        ] {
            assert_eq!(scalar_i64(&conn, sql), 0, "{sql}");
        }
        assert!(!path.exists());
    }

    #[test]
    fn delete_removes_file_attachment_after_database_commit() {
        let mut conn = fixture_with_all_kinds();
        let path = temp_asset("file-delete");
        std::fs::write(&path, b"file asset").unwrap();
        conn.execute(
            "UPDATE entries SET file_path=?1 WHERE id='file-1'",
            params![path.to_string_lossy()],
        )
        .unwrap();

        delete(&mut conn, "dock:file-1").unwrap();

        assert_eq!(
            scalar_i64(&conn, "SELECT COUNT(*) FROM entries WHERE id='file-1'"),
            0
        );
        assert!(!path.exists());
    }

    #[test]
    fn conditional_delete_rechecks_temporary_retention_after_stale_intent() {
        let database_path = temp_asset("conditional-delete-db");
        let mut first = Connection::open(&database_path).unwrap();
        first.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::scratchpad::storage::ensure_dock_schema(&mut first).unwrap();
        crate::vault::storage::ensure_vault_schema(&mut first).unwrap();
        crate::content::migrations::ensure_content_schema(&mut first, 7).unwrap();
        let entry = crate::scratchpad::storage::create_text_entry(
            &mut first,
            crate::models::entry::EntryView::Home,
            "stale intent",
            "manual",
        )
        .unwrap();
        let unified_id = format!("dock:{}", entry.id);
        assert_eq!(
            first
                .query_row(
                    "SELECT retention_state FROM content_catalog WHERE unified_id=?1",
                    params![unified_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "temporary"
        );

        let mut second = Connection::open(&database_path).unwrap();
        second.pragma_update(None, "foreign_keys", "ON").unwrap();
        save(&mut second, &unified_id).unwrap();
        let saved_revision = revision(&second);

        assert!(matches!(
            delete_temporary(&mut first, &unified_id),
            Err(StorageError::Validation(_))
        ));

        assert_eq!(revision(&first), saved_revision);
        for sql in [
            "SELECT COUNT(*) FROM entries WHERE id=?1",
            "SELECT COUNT(*) FROM home_entries WHERE entry_id=?1",
            "SELECT COUNT(*) FROM note_entries WHERE entry_id=?1",
        ] {
            assert_eq!(
                first
                    .query_row(sql, params![entry.id], |row| row.get::<_, i64>(0))
                    .unwrap(),
                1,
                "{sql}"
            );
        }
        for table in ["content_catalog", "content_fts"] {
            assert_eq!(
                first
                    .query_row(
                        &format!("SELECT COUNT(*) FROM {table} WHERE unified_id=?1"),
                        params![unified_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                1,
                "{table}"
            );
        }
        drop(second);
        drop(first);
        std::fs::remove_file(database_path).unwrap();
    }

    #[test]
    fn conditional_delete_removes_a_still_temporary_dock_item() {
        let mut conn = fixture_with_all_kinds();
        let start = revision(&conn);

        let mutation = delete_temporary(&mut conn, "dock:text-1").unwrap();

        assert_eq!(mutation.revision, start + 1);
        assert_eq!(
            mutation.changes[0].operation,
            crate::content::models::ContentOperation::Deleted
        );
        for sql in [
            "SELECT COUNT(*) FROM entries WHERE id='text-1'",
            "SELECT COUNT(*) FROM home_entries WHERE entry_id='text-1'",
            "SELECT COUNT(*) FROM content_catalog WHERE unified_id='dock:text-1'",
            "SELECT COUNT(*) FROM content_fts WHERE unified_id='dock:text-1'",
        ] {
            assert_eq!(scalar_i64(&conn, sql), 0, "{sql}");
        }
    }

    #[test]
    fn delete_vault_explicitly_removes_all_children_and_both_search_indexes() {
        let mut conn = fixture_with_all_kinds();
        conn.execute(
            "INSERT INTO vault_capture_requests(request_id, entry_id, created_at)
             VALUES ('capture-delete', 'credential-1', '2026-07-18T07:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vault_fts(entry_id, title, notes, searchable)
             VALUES ('credential-1', 'Production login', '', 'alice')",
            [],
        )
        .unwrap();
        let start = revision(&conn);

        let mutation = delete(&mut conn, "vault:credential-1").unwrap();

        assert_eq!(mutation.revision, start + 1);
        for sql in [
            "SELECT COUNT(*) FROM vault_capture_requests WHERE entry_id='credential-1'",
            "SELECT COUNT(*) FROM vault_ai_metadata WHERE entry_id='credential-1'",
            "SELECT COUNT(*) FROM vault_tags WHERE entry_id='credential-1'",
            "SELECT COUNT(*) FROM vault_fields WHERE entry_id='credential-1'",
            "SELECT COUNT(*) FROM vault_fts WHERE entry_id='credential-1'",
            "SELECT COUNT(*) FROM vault_entries WHERE id='credential-1'",
            "SELECT COUNT(*) FROM content_fts WHERE unified_id='vault:credential-1'",
            "SELECT COUNT(*) FROM content_catalog WHERE unified_id='vault:credential-1'",
        ] {
            assert_eq!(scalar_i64(&conn, sql), 0, "{sql}");
        }
    }

    #[test]
    fn delete_database_failure_rolls_back_everything_and_keeps_attachment() {
        let mut conn = fixture_with_all_kinds();
        let path = temp_asset("dock-rollback");
        std::fs::write(&path, b"asset").unwrap();
        conn.execute(
            "UPDATE entries SET file_path=?1 WHERE id='image-1'",
            params![path.to_string_lossy()],
        )
        .unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_catalog_delete BEFORE DELETE ON content_catalog
             WHEN OLD.unified_id='dock:image-1'
             BEGIN SELECT RAISE(ABORT, 'forced delete failure'); END;",
        )
        .unwrap();
        let start = revision(&conn);

        assert!(matches!(
            delete(&mut conn, "dock:image-1"),
            Err(StorageError::Sqlite(_))
        ));

        assert_eq!(revision(&conn), start);
        assert_eq!(
            scalar_i64(&conn, "SELECT COUNT(*) FROM entries WHERE id='image-1'"),
            1
        );
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM home_entries WHERE entry_id='image-1'"
            ),
            1
        );
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM content_fts WHERE unified_id='dock:image-1'"
            ),
            1
        );
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM content_catalog WHERE unified_id='dock:image-1'"
            ),
            1
        );
        assert!(path.is_file());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn deleting_text_never_removes_legacy_file_path() {
        let mut conn = fixture_with_all_kinds();
        let sentinel = temp_asset("text-delete-sentinel");
        std::fs::write(&sentinel, b"must survive text deletion").unwrap();
        conn.execute(
            "UPDATE entries SET file_path=?1 WHERE id='text-1'",
            params![sentinel.to_string_lossy()],
        )
        .unwrap();

        delete(&mut conn, "dock:text-1").unwrap();

        assert_eq!(
            scalar_i64(&conn, "SELECT COUNT(*) FROM entries WHERE id='text-1'"),
            0
        );
        assert!(sentinel.is_file());
        std::fs::remove_file(sentinel).unwrap();
    }

    #[test]
    fn attachment_cleanup_failure_after_commit_does_not_restore_database_rows() {
        let mut conn = fixture_with_all_kinds();
        let path = temp_asset("cleanup-failure");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("child"), b"keep directory non-empty").unwrap();
        conn.execute(
            "UPDATE entries SET file_path=?1 WHERE id='image-1'",
            params![path.to_string_lossy()],
        )
        .unwrap();

        delete(&mut conn, "dock:image-1").unwrap();

        assert_eq!(
            scalar_i64(&conn, "SELECT COUNT(*) FROM entries WHERE id='image-1'"),
            0
        );
        assert_eq!(
            scalar_i64(
                &conn,
                "SELECT COUNT(*) FROM content_catalog WHERE unified_id='dock:image-1'"
            ),
            0
        );
        assert!(path.is_dir());
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn cleanup_expired_deletes_only_due_unprotected_rows_and_bumps_once() {
        let mut conn = fixture_with_all_kinds();
        conn.execute_batch(
            "UPDATE content_catalog
             SET retention_state='temporary', saved_position=NULL, inbox_position=10.0,
                 cleanup_at='2026-07-18T10:00:00+02:00'
             WHERE unified_id='dock:text-1';
             UPDATE content_catalog
             SET cleanup_at='2026-07-18T07:59:00Z'
             WHERE unified_id='dock:image-1';
             UPDATE content_catalog
             SET cleanup_at='2020-01-01T00:00:00Z'
             WHERE unified_id='dock:file-1';
             UPDATE content_catalog
             SET retention_state='temporary', saved_position=NULL, inbox_position=20.0,
                 cleanup_at='2026-07-18T08:01:00Z'
             WHERE unified_id='vault:credential-1';
             UPDATE content_catalog
             SET retention_state='temporary', saved_position=NULL, inbox_position=21.0,
                 cleanup_at=NULL
             WHERE unified_id='vault:bookmark-1';
             UPDATE content_catalog
             SET retention_state='temporary', saved_position=NULL, inbox_position=22.0,
                 cleanup_at='2026-07-18T07:00:00Z'
             WHERE unified_id='vault:note-1';
             INSERT INTO content_pending_deletes(
                 token, unified_id, created_at, expires_at, status
             ) VALUES ('protect-note', 'vault:note-1', '2026-07-18T06:00:00Z',
                       '2026-07-18T07:00:00Z', 'failed');",
        )
        .unwrap();
        let start = revision(&conn);
        let now = DateTime::parse_from_rfc3339("2026-07-18T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let mutation = cleanup_expired(&mut conn, now).unwrap();

        assert_eq!(mutation.value, 2);
        assert_eq!(mutation.revision, start + 1);
        assert_eq!(
            mutation
                .changes
                .iter()
                .map(|change| change.id.as_str())
                .collect::<Vec<_>>(),
            vec!["dock:image-1", "dock:text-1"]
        );
        for id in ["dock:image-1", "dock:text-1"] {
            assert_eq!(
                conn.query_row(
                    "SELECT COUNT(*) FROM content_catalog WHERE unified_id=?1",
                    params![id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
                0
            );
        }
        for id in [
            "dock:file-1",
            "vault:credential-1",
            "vault:bookmark-1",
            "vault:note-1",
        ] {
            assert_eq!(
                conn.query_row(
                    "SELECT COUNT(*) FROM content_catalog WHERE unified_id=?1",
                    params![id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
                1,
                "{id}"
            );
        }

        let noop = cleanup_expired(&mut conn, now).unwrap();
        assert_eq!(noop.value, 0);
        assert_eq!(noop.revision, start + 1);
        assert!(noop.changes.is_empty());
    }

    #[test]
    fn cleanup_bad_timestamp_rolls_back_all_due_rows_and_revision() {
        let mut conn = fixture_with_all_kinds();
        conn.execute_batch(
            "UPDATE content_catalog SET cleanup_at='2026-07-18T07:00:00Z'
             WHERE unified_id='dock:text-1';
             UPDATE content_catalog SET cleanup_at='not-a-timestamp'
             WHERE unified_id='dock:image-1';",
        )
        .unwrap();
        let start = revision(&conn);
        let now = DateTime::parse_from_rfc3339("2026-07-18T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert!(matches!(
            cleanup_expired(&mut conn, now),
            Err(StorageError::Validation(_))
        ));

        assert_eq!(revision(&conn), start);
        for id in ["dock:text-1", "dock:image-1"] {
            assert_eq!(
                conn.query_row(
                    "SELECT COUNT(*) FROM content_catalog WHERE unified_id=?1",
                    params![id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
                1
            );
        }
    }

    #[test]
    fn cleanup_of_expired_text_never_removes_legacy_file_path() {
        let mut conn = fixture_with_all_kinds();
        let sentinel = temp_asset("text-cleanup-sentinel");
        std::fs::write(&sentinel, b"must survive text cleanup").unwrap();
        conn.execute(
            "UPDATE entries SET file_path=?1 WHERE id='text-1'",
            params![sentinel.to_string_lossy()],
        )
        .unwrap();
        conn.execute(
            "UPDATE content_catalog SET cleanup_at='2020-01-01T00:00:00Z'
             WHERE unified_id='dock:text-1'",
            [],
        )
        .unwrap();
        let now = DateTime::parse_from_rfc3339("2020-01-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let mutation = cleanup_expired(&mut conn, now).unwrap();

        assert_eq!(mutation.value, 1);
        assert_eq!(
            scalar_i64(&conn, "SELECT COUNT(*) FROM entries WHERE id='text-1'"),
            0
        );
        assert!(sentinel.is_file());
        std::fs::remove_file(sentinel).unwrap();
    }
}
