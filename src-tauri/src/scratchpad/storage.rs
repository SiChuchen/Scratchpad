use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::content::catalog::{bump_revision, top_position};
use crate::content::models::{
    ContentChange, ContentMutation, ContentOperation, ContentSource, RetentionState,
    UnifiedContentId,
};
use crate::content::projection::{build_search_document, replace_projection};
use crate::models::entry::{DockEntry, EntryKind, EntryView};
use crate::models::scratchpad::ScratchpadItem;
use crate::storage::error::{StorageError, StorageResult};
use crate::storage::migration::{ensure_schema, Migration};

const DOCK_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS entries (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('text', 'image', 'file')),
    content TEXT,
    file_path TEXT,
    file_name TEXT,
    mime_type TEXT,
    width INTEGER,
    height INTEGER,
    size_bytes INTEGER,
    collapsed INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL DEFAULT 'manual',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS home_entries (
    entry_id TEXT PRIMARY KEY REFERENCES entries(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    sort_order REAL NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS note_entries (
    entry_id TEXT PRIMARY KEY REFERENCES entries(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    sort_order REAL NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_home_entries_created_at
    ON home_entries(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_note_entries_created_at
    ON note_entries(created_at DESC);

CREATE TABLE IF NOT EXISTS preferences (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn new_entry_id() -> String {
    format!(
        "de-{}",
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}

fn parse_entry_kind(value: &str) -> rusqlite::Result<EntryKind> {
    match value {
        "text" => Ok(EntryKind::Text),
        "image" => Ok(EntryKind::Image),
        "file" => Ok(EntryKind::File),
        other => Err(rusqlite::Error::InvalidParameterName(format!(
            "unknown entry kind: {other}"
        ))),
    }
}

fn row_to_dock_entry(row: &Row) -> rusqlite::Result<DockEntry> {
    let kind: String = row.get("kind")?;
    Ok(DockEntry {
        id: row.get("id")?,
        kind: parse_entry_kind(&kind)?,
        content: row.get("content")?,
        file_path: row.get("file_path")?,
        file_name: row.get("file_name")?,
        mime_type: row.get("mime_type")?,
        width: row.get("width")?,
        height: row.get("height")?,
        size_bytes: row.get("size_bytes")?,
        collapsed: row.get::<_, i32>("collapsed")? != 0,
        title: row.get("title")?,
        in_home: row.get::<_, i32>("in_home")? != 0,
        in_note: row.get::<_, i32>("in_note")? != 0,
        source: row.get("source")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_item(row: &Row) -> rusqlite::Result<ScratchpadItem> {
    Ok(ScratchpadItem {
        id: row.get("id")?,
        item_type: row.get("item_type")?,
        content: row.get("content")?,
        file_path: row.get("file_path")?,
        file_name: row.get("file_name")?,
        mime_type: row.get("mime_type")?,
        width: row.get("width")?,
        height: row.get("height")?,
        size_bytes: row.get("size_bytes")?,
        pinned: row.get::<_, i32>("pinned")? != 0,
        source: row.get("source")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn insert_entry_row(
    conn: &Connection,
    entry: &DockEntry,
    ignore_existing: bool,
) -> StorageResult<()> {
    let sql = if ignore_existing {
        "INSERT OR IGNORE INTO entries (
            id, kind, content, file_path, file_name, mime_type,
            width, height, size_bytes, collapsed, source, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
    } else {
        "INSERT INTO entries (
            id, kind, content, file_path, file_name, mime_type,
            width, height, size_bytes, collapsed, source, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
    };
    conn.execute(
        sql,
        params![
            entry.id,
            entry.kind.as_str(),
            entry.content,
            entry.file_path,
            entry.file_name,
            entry.mime_type,
            entry.width,
            entry.height,
            entry.size_bytes,
            entry.collapsed as i32,
            entry.source,
            entry.created_at,
            entry.updated_at,
        ],
    )?;
    Ok(())
}

fn insert_membership_row(
    conn: &Connection,
    view: EntryView,
    entry_id: &str,
    created_at: &str,
    ignore_existing: bool,
) -> StorageResult<()> {
    let table = view.membership_table();
    let sql = if ignore_existing {
        format!("INSERT OR IGNORE INTO {table} (entry_id, created_at) VALUES (?1, ?2)")
    } else {
        format!("INSERT INTO {table} (entry_id, created_at) VALUES (?1, ?2)")
    };
    conn.execute(&sql, params![entry_id, created_at])?;
    Ok(())
}

fn insert_membership_row_at_position(
    conn: &Connection,
    view: EntryView,
    entry_id: &str,
    created_at: &str,
    position: f64,
) -> StorageResult<()> {
    let table = view.membership_table();
    conn.execute(
        &format!("INSERT INTO {table} (entry_id, created_at, sort_order) VALUES (?1, ?2, ?3)"),
        params![entry_id, created_at, position],
    )?;
    Ok(())
}

fn content_schema_exists(conn: &Connection) -> StorageResult<bool> {
    table_exists(conn, "content_state")
}

fn dock_unified_id(entry_id: &str) -> StorageResult<String> {
    Ok(UnifiedContentId::new(ContentSource::Dock, entry_id)
        .map_err(StorageError::Validation)?
        .as_str()
        .to_string())
}

fn cleanup_at(retention_changed_at: &str, cleanup_days: i64) -> StorageResult<String> {
    let cleanup_delta = crate::content::migrations::validate_cleanup_days(cleanup_days)?;
    let changed_at = DateTime::parse_from_rfc3339(retention_changed_at).map_err(|error| {
        StorageError::Validation(format!(
            "invalid Dock retention timestamp {retention_changed_at:?}: {error}"
        ))
    })?;
    changed_at
        .checked_add_signed(cleanup_delta)
        .map(|timestamp| timestamp.with_timezone(&Utc).to_rfc3339())
        .ok_or_else(|| {
            StorageError::Validation(format!(
                "Dock cleanup timestamp is out of range for {retention_changed_at:?}"
            ))
        })
}

fn upsert_dock_projection_in_transaction(
    conn: &Connection,
    entry_id: &str,
    cleanup_days: i64,
) -> StorageResult<String> {
    let row = conn.query_row(
        "SELECT e.kind, e.created_at, e.updated_at,
                n.entry_id IS NOT NULL,
                COALESCE(n.created_at, h.created_at, e.created_at),
                h.sort_order, n.sort_order
         FROM entries e
         LEFT JOIN home_entries h ON h.entry_id=e.id
         LEFT JOIN note_entries n ON n.entry_id=e.id
         WHERE e.id=?1",
        params![entry_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<f64>>(5)?,
                row.get::<_, Option<f64>>(6)?,
            ))
        },
    )?;
    let (kind, created_at, updated_at, saved, retention_changed_at, home_position, note_position) =
        row;
    let unified_id = dock_unified_id(entry_id)?;
    let (retention, cleanup, inbox_position, saved_position) = if saved {
        ("saved", None, None, note_position)
    } else {
        (
            "temporary",
            Some(cleanup_at(&retention_changed_at, cleanup_days)?),
            home_position,
            None,
        )
    };
    conn.execute(
        "INSERT INTO content_catalog(
             unified_id, source, source_id, kind, retention_state,
             retention_changed_at, cleanup_at, inbox_position, saved_position,
             created_at, updated_at
         ) VALUES (?1, 'dock', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(unified_id) DO UPDATE SET
             source=excluded.source,
             source_id=excluded.source_id,
             kind=excluded.kind,
             created_at=excluded.created_at,
             updated_at=excluded.updated_at",
        params![
            unified_id,
            entry_id,
            kind,
            retention,
            retention_changed_at,
            cleanup,
            inbox_position,
            saved_position,
            created_at,
            updated_at,
        ],
    )?;
    let document = build_search_document(conn, &unified_id)?;
    replace_projection(conn, &document)?;
    Ok(unified_id)
}

fn membership_count(conn: &Connection, entry_id: &str) -> StorageResult<i64> {
    let count: i64 = conn.query_row(
        r#"
        SELECT
            (SELECT COUNT(*) FROM home_entries WHERE entry_id = ?1)
            + (SELECT COUNT(*) FROM note_entries WHERE entry_id = ?1)
        "#,
        params![entry_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

fn delete_orphaned_entry_internal(
    conn: &Connection,
    entry_id: &str,
) -> StorageResult<(bool, Option<String>)> {
    if membership_count(conn, entry_id)? == 0 {
        let file_path: Option<String> = conn
            .query_row(
                "SELECT CASE WHEN kind IN ('image', 'file') THEN file_path ELSE NULL END
                 FROM entries WHERE id = ?1",
                params![entry_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let rows = conn.execute("DELETE FROM entries WHERE id = ?1", params![entry_id])?;
        return Ok((rows > 0, file_path));
    }
    Ok((false, None))
}

fn remove_attachment_after_commit(entry_id: &str, attachment: Option<String>) {
    if let Some(attachment) = attachment {
        if let Err(error) = fs::remove_file(Path::new(&attachment)) {
            eprintln!("failed to remove attachment for dock:{entry_id}: {error}");
        }
    }
}

fn remove_membership_row(
    conn: &Connection,
    view: EntryView,
    entry_id: &str,
) -> StorageResult<usize> {
    let table = view.membership_table();
    let rows = conn.execute(
        &format!("DELETE FROM {table} WHERE entry_id = ?1"),
        params![entry_id],
    )?;
    Ok(rows)
}

fn list_entries_internal(
    conn: &Connection,
    view: EntryView,
    kind: Option<EntryKind>,
) -> StorageResult<Vec<DockEntry>> {
    let mut sql = format!(
        r#"
        SELECT
            e.id,
            e.kind,
            e.content,
            e.file_path,
            e.file_name,
            e.mime_type,
            e.width,
            e.height,
            e.size_bytes,
            e.collapsed,
            e.title,
            e.source,
            e.created_at,
            e.updated_at,
            CASE WHEN h.entry_id IS NULL THEN 0 ELSE 1 END AS in_home,
            CASE WHEN n.entry_id IS NULL THEN 0 ELSE 1 END AS in_note
        FROM {membership_table} m
        JOIN entries e ON e.id = m.entry_id
        LEFT JOIN home_entries h ON h.entry_id = e.id
        LEFT JOIN note_entries n ON n.entry_id = e.id
        "#,
        membership_table = view.membership_table()
    );

    if kind.is_some() {
        sql.push_str(" WHERE e.kind = ?1");
    }
    sql.push_str(" ORDER BY m.sort_order ASC, m.created_at DESC, e.id DESC");

    let mut stmt = conn.prepare(&sql)?;
    let entries = if let Some(kind) = kind {
        stmt.query_map(params![kind.as_str()], row_to_dock_entry)?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map([], row_to_dock_entry)?
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(entries)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn create_dock_entry_internal(
    conn: &mut Connection,
    view: EntryView,
    kind: EntryKind,
    content: Option<String>,
    file_path: Option<String>,
    file_name: Option<String>,
    mime_type: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    size_bytes: Option<i64>,
    source: &str,
) -> StorageResult<DockEntry> {
    if content_schema_exists(conn)? {
        return Ok(create_dock_entry_internal_with_revision(
            conn, view, kind, content, file_path, file_name, mime_type, width, height, size_bytes,
            source,
        )?
        .value);
    }

    create_dock_entry_without_projection(
        conn, view, kind, content, file_path, file_name, mime_type, width, height, size_bytes,
        source,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn create_dock_entry_internal_with_revision(
    conn: &mut Connection,
    view: EntryView,
    kind: EntryKind,
    content: Option<String>,
    file_path: Option<String>,
    file_name: Option<String>,
    mime_type: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    size_bytes: Option<i64>,
    source: &str,
) -> StorageResult<ContentMutation<DockEntry>> {
    let id = new_entry_id();
    let now = now_rfc3339();
    let entry = DockEntry {
        id: id.clone(),
        kind,
        content,
        file_path,
        file_name,
        mime_type,
        width,
        height,
        size_bytes,
        collapsed: false,
        title: None,
        in_home: matches!(view, EntryView::Home),
        in_note: matches!(view, EntryView::Note),
        source: source.to_string(),
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    let tx = conn.transaction()?;
    let retention = match view {
        EntryView::Home => RetentionState::Temporary,
        EntryView::Note => RetentionState::Saved,
    };
    let position = top_position(&tx, retention)?;
    let cleanup_days = crate::scratchpad::preferences::load_preferences(&tx)?.auto_cleanup_days;
    insert_entry_row(&tx, &entry, false)?;
    insert_membership_row_at_position(&tx, view, &entry.id, &entry.created_at, position)?;
    let unified_id = upsert_dock_projection_in_transaction(&tx, &entry.id, cleanup_days)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;

    Ok(ContentMutation {
        value: entry,
        revision,
        changes: vec![ContentChange {
            id: unified_id,
            operation: ContentOperation::Created,
        }],
    })
}

#[allow(clippy::too_many_arguments)]
fn create_dock_entry_without_projection(
    conn: &mut Connection,
    view: EntryView,
    kind: EntryKind,
    content: Option<String>,
    file_path: Option<String>,
    file_name: Option<String>,
    mime_type: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    size_bytes: Option<i64>,
    source: &str,
) -> StorageResult<DockEntry> {
    let id = new_entry_id();
    let now = now_rfc3339();
    let entry = DockEntry {
        id: id.clone(),
        kind,
        content,
        file_path,
        file_name,
        mime_type,
        width,
        height,
        size_bytes,
        collapsed: false,
        title: None,
        in_home: matches!(view, EntryView::Home),
        in_note: matches!(view, EntryView::Note),
        source: source.to_string(),
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    let tx = conn.transaction()?;
    insert_entry_row(&tx, &entry, false)?;
    insert_membership_row(&tx, view, &entry.id, &entry.created_at, false)?;
    tx.commit()?;

    Ok(entry)
}

fn entry_exists(conn: &Connection, entry_id: &str) -> StorageResult<bool> {
    let exists: i64 = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM entries WHERE id = ?1)",
        params![entry_id],
        |row| row.get(0),
    )?;
    Ok(exists != 0)
}

fn home_only_entry_ids(conn: &Connection, max_age_days: i64) -> StorageResult<Vec<String>> {
    let sql = if max_age_days <= 0 {
        r#"
        SELECT h.entry_id
        FROM home_entries h
        LEFT JOIN note_entries n ON n.entry_id = h.entry_id
        WHERE n.entry_id IS NULL
        ORDER BY h.created_at DESC
        "#
    } else {
        r#"
        SELECT h.entry_id
        FROM home_entries h
        LEFT JOIN note_entries n ON n.entry_id = h.entry_id
        WHERE n.entry_id IS NULL
          AND h.created_at <= datetime('now', ?1)
        ORDER BY h.created_at DESC
        "#
    };
    let mut stmt = conn.prepare(sql)?;
    let ids = if max_age_days > 0 {
        stmt.query_map(params![format!("-{} days", max_age_days)], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?
    } else {
        stmt.query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?
    };
    Ok(ids)
}

fn table_exists(conn: &Connection, table: &str) -> StorageResult<bool> {
    let exists: i64 = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        params![table],
        |row| row.get(0),
    )?;
    Ok(exists != 0)
}

fn migrate_legacy_scratchpad_items(conn: &mut Connection) -> StorageResult<usize> {
    if !table_exists(conn, "scratchpad_items")? {
        return Ok(0);
    }

    let legacy_items = {
        let mut stmt = conn.prepare("SELECT * FROM scratchpad_items ORDER BY created_at DESC")?;
        let items = stmt
            .query_map([], row_to_item)?
            .collect::<Result<Vec<_>, _>>()?;
        items
    };

    let tx = conn.transaction()?;
    for item in &legacy_items {
        let kind = parse_entry_kind(&item.item_type)?;
        let entry = DockEntry {
            id: item.id.clone(),
            kind,
            content: item.content.clone(),
            file_path: item.file_path.clone(),
            file_name: item.file_name.clone(),
            mime_type: item.mime_type.clone(),
            width: item.width,
            height: item.height,
            size_bytes: item.size_bytes,
            collapsed: item.pinned,
            title: None,
            in_home: true,
            in_note: true,
            source: item.source.clone(),
            created_at: item.created_at.clone(),
            updated_at: item.updated_at.clone(),
        };

        insert_entry_row(&tx, &entry, true)?;
        insert_membership_row(&tx, EntryView::Home, &entry.id, &entry.created_at, true)?;
        insert_membership_row(&tx, EntryView::Note, &entry.id, &entry.created_at, true)?;
    }
    tx.commit()?;

    Ok(legacy_items.len())
}

pub fn dock_migrations() -> Vec<Migration> {
    vec![
        Migration::new(1, "create dock schema", DOCK_SCHEMA_SQL),
        Migration::new(
            2,
            "add title column",
            "ALTER TABLE entries ADD COLUMN title TEXT",
        ),
    ]
}

pub fn ensure_dock_schema(conn: &mut Connection) -> StorageResult<()> {
    ensure_schema(conn, &dock_migrations())?;
    conn.execute_batch(DOCK_SCHEMA_SQL)?;
    migrate_legacy_scratchpad_items(conn)?;
    Ok(())
}

pub fn list_entries(
    conn: &Connection,
    view: EntryView,
    kind: Option<EntryKind>,
) -> StorageResult<Vec<DockEntry>> {
    list_entries_internal(conn, view, kind)
}

pub fn add_to_note(conn: &mut Connection, entry_id: &str) -> StorageResult<()> {
    if content_schema_exists(conn)? {
        add_to_note_with_revision(conn, entry_id)?;
        return Ok(());
    }

    if !entry_exists(conn, entry_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }

    let now = now_rfc3339();
    let tx = conn.transaction()?;
    insert_membership_row(&tx, EntryView::Note, entry_id, &now, true)?;
    tx.commit()?;
    Ok(())
}

pub fn add_to_note_with_revision(
    conn: &mut Connection,
    entry_id: &str,
) -> StorageResult<ContentMutation<()>> {
    if !entry_exists(conn, entry_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    let unified_id = dock_unified_id(entry_id)?;
    let catalog = crate::content::catalog::catalog_entry_by_id(conn, &unified_id)?;
    if catalog.source != ContentSource::Dock || catalog.source_id != entry_id {
        return Err(StorageError::Validation(format!(
            "Dock catalog identity mismatch for {unified_id}"
        )));
    }
    if catalog.retention == RetentionState::Saved {
        let in_note: i64 = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM note_entries WHERE entry_id=?1)",
            params![entry_id],
            |row| row.get(0),
        )?;
        if in_note == 0 {
            return Err(StorageError::Validation(format!(
                "saved Dock content is missing Note membership: {unified_id}"
            )));
        }
        return Ok(ContentMutation {
            value: (),
            revision: crate::content::catalog::current_revision(conn)?,
            changes: Vec::new(),
        });
    }
    Ok(unit_mutation(crate::content::service::save(
        conn,
        &unified_id,
    )?))
}

pub fn remove_from_view(
    conn: &mut Connection,
    view: EntryView,
    entry_id: &str,
) -> StorageResult<()> {
    if content_schema_exists(conn)? {
        remove_from_view_with_revision(conn, view, entry_id)?;
        return Ok(());
    }

    let tx = conn.transaction()?;
    let rows = remove_membership_row(&tx, view, entry_id)?;
    if rows == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    let (_, attachment) = delete_orphaned_entry_internal(&tx, entry_id)?;
    tx.commit()?;
    remove_attachment_after_commit(entry_id, attachment);
    Ok(())
}

pub fn remove_from_view_with_revision(
    conn: &mut Connection,
    view: EntryView,
    entry_id: &str,
) -> StorageResult<ContentMutation<()>> {
    let table = view.membership_table();
    let present: i64 = conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE entry_id=?1)"),
        params![entry_id],
        |row| row.get(0),
    )?;
    if present == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }

    let unified_id = dock_unified_id(entry_id)?;
    let catalog = crate::content::catalog::catalog_entry_by_id(conn, &unified_id)?;
    if catalog.source != ContentSource::Dock || catalog.source_id != entry_id {
        return Err(StorageError::Validation(format!(
            "Dock catalog identity mismatch for {unified_id}"
        )));
    }

    match (view, catalog.retention) {
        (EntryView::Note, RetentionState::Saved) => {
            let cleanup_days =
                crate::scratchpad::preferences::load_preferences(conn)?.auto_cleanup_days;
            Ok(unit_mutation(crate::content::service::unsave(
                conn,
                &unified_id,
                cleanup_days,
            )?))
        }
        (EntryView::Note, RetentionState::Temporary) => Err(StorageError::Validation(format!(
            "temporary Dock content cannot have Note membership: {unified_id}"
        ))),
        (EntryView::Home, RetentionState::Temporary) => {
            if membership_count(conn, entry_id)? != 1 {
                return Err(StorageError::Validation(format!(
                    "temporary Dock content has inconsistent memberships: {unified_id}"
                )));
            }
            crate::content::service::delete(conn, &unified_id)
        }
        (EntryView::Home, RetentionState::Saved) => {
            let tx = conn.transaction()?;
            let rows = remove_membership_row(&tx, EntryView::Home, entry_id)?;
            if rows != 1 {
                return Err(rusqlite::Error::QueryReturnedNoRows.into());
            }
            let affected = tx.execute(
                "UPDATE content_catalog SET inbox_position=NULL
                 WHERE unified_id=?1 AND retention_state='saved'",
                params![unified_id],
            )?;
            if affected != 1 {
                return Err(StorageError::Validation(format!(
                    "saved Dock catalog row disappeared: {unified_id}"
                )));
            }
            let revision = bump_revision(&tx)?;
            tx.commit()?;
            Ok(ContentMutation {
                value: (),
                revision,
                changes: vec![ContentChange {
                    id: unified_id,
                    operation: ContentOperation::Retention,
                }],
            })
        }
    }
}

fn unit_mutation<T>(mutation: ContentMutation<T>) -> ContentMutation<()> {
    ContentMutation {
        value: (),
        revision: mutation.revision,
        changes: mutation.changes,
    }
}

pub fn reorder_entries(
    conn: &mut Connection,
    view: EntryView,
    ordered_ids: &[String],
) -> StorageResult<()> {
    if content_schema_exists(conn)? {
        reorder_entries_with_revision(conn, view, ordered_ids)?;
        return Ok(());
    }

    let table = view.membership_table();
    let tx = conn.transaction()?;
    for (i, id) in ordered_ids.iter().enumerate() {
        let order = i as f64;
        tx.execute(
            &format!("UPDATE {table} SET sort_order = ?1 WHERE entry_id = ?2"),
            params![order, id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn reorder_entries_with_revision(
    conn: &mut Connection,
    view: EntryView,
    ordered_ids: &[String],
) -> StorageResult<ContentMutation<()>> {
    let supplied = ordered_ids.iter().collect::<BTreeSet<_>>();
    if supplied.len() != ordered_ids.len() {
        return Err(StorageError::Validation(
            "reorder IDs must not contain duplicates".to_string(),
        ));
    }

    let table = view.membership_table();
    let position_column = match view {
        EntryView::Home => "inbox_position",
        EntryView::Note => "saved_position",
    };
    let tx = conn.transaction()?;
    let current_ids = {
        let mut stmt = tx.prepare(&format!("SELECT entry_id FROM {table}"))?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids
    };
    let current = current_ids.iter().collect::<BTreeSet<_>>();
    if supplied != current {
        return Err(StorageError::Validation(
            "reorder IDs must exactly match the current Dock view".to_string(),
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

    let mut unified_ids = Vec::with_capacity(ordered_ids.len());
    for (index, entry_id) in ordered_ids.iter().enumerate() {
        let unified_id = dock_unified_id(entry_id)?;
        let catalog = crate::content::catalog::catalog_entry_by_id(&tx, &unified_id)?;
        if catalog.source != ContentSource::Dock || catalog.source_id != *entry_id {
            return Err(StorageError::Validation(format!(
                "Dock catalog identity mismatch for {unified_id}"
            )));
        }
        if view == EntryView::Note && catalog.retention != RetentionState::Saved {
            return Err(StorageError::Validation(format!(
                "Note reorder includes non-saved content: {unified_id}"
            )));
        }
        let position = index as f64;
        let membership_rows = tx.execute(
            &format!("UPDATE {table} SET sort_order=?2 WHERE entry_id=?1"),
            params![entry_id, position],
        )?;
        if membership_rows != 1 {
            return Err(StorageError::Validation(format!(
                "Dock membership disappeared while reordering {unified_id}"
            )));
        }
        let catalog_rows = tx.execute(
            &format!("UPDATE content_catalog SET {position_column}=?2 WHERE unified_id=?1"),
            params![unified_id, position],
        )?;
        if catalog_rows != 1 {
            return Err(StorageError::Validation(format!(
                "Dock catalog row disappeared while reordering {unified_id}"
            )));
        }
        unified_ids.push(unified_id);
    }
    let revision = bump_revision(&tx)?;
    tx.commit()?;

    Ok(ContentMutation {
        value: (),
        revision,
        changes: unified_ids
            .into_iter()
            .map(|id| ContentChange {
                id,
                operation: ContentOperation::Reordered,
            })
            .collect(),
    })
}

pub fn cleanup_home_on_startup(conn: &mut Connection, max_age_days: i64) -> StorageResult<usize> {
    let tx = conn.transaction()?;
    let ids = home_only_entry_ids(&tx, max_age_days)?;
    let mut deleted = 0usize;
    let mut attachments = Vec::new();

    for entry_id in ids {
        let existed = entry_exists(&tx, &entry_id)?;
        remove_membership_row(&tx, EntryView::Home, &entry_id)?;
        let (removed, attachment) = delete_orphaned_entry_internal(&tx, &entry_id)?;
        if existed && removed {
            deleted += 1;
            attachments.push((entry_id, attachment));
        }
    }

    tx.commit()?;
    for (entry_id, attachment) in attachments {
        remove_attachment_after_commit(&entry_id, attachment);
    }
    Ok(deleted)
}

pub fn delete_orphaned_entry(conn: &mut Connection, entry_id: &str) -> StorageResult<()> {
    let (_, attachment) = delete_orphaned_entry_internal(conn, entry_id)?;
    remove_attachment_after_commit(entry_id, attachment);
    Ok(())
}

pub fn create_text_entry(
    conn: &mut Connection,
    view: EntryView,
    content: &str,
    source: &str,
) -> StorageResult<DockEntry> {
    create_dock_entry_internal(
        conn,
        view,
        EntryKind::Text,
        Some(content.to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
        source,
    )
}

pub fn create_text_entry_with_revision(
    conn: &mut Connection,
    view: EntryView,
    content: &str,
    source: &str,
) -> StorageResult<ContentMutation<DockEntry>> {
    create_dock_entry_internal_with_revision(
        conn,
        view,
        EntryKind::Text,
        Some(content.to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
        source,
    )
}

pub fn update_entry_text(conn: &mut Connection, id: &str, content: &str) -> StorageResult<()> {
    if content_schema_exists(conn)? {
        update_entry_text_with_revision(conn, id, content)?;
        return Ok(());
    }

    let now = now_rfc3339();
    let rows = conn.execute(
        "UPDATE entries SET content = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, content, now],
    )?;
    if rows == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    Ok(())
}

pub fn update_entry_text_with_revision(
    conn: &mut Connection,
    id: &str,
    content: &str,
) -> StorageResult<ContentMutation<()>> {
    let now = now_rfc3339();
    let tx = conn.transaction()?;
    let rows = tx.execute(
        "UPDATE entries SET content = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, content, now],
    )?;
    if rows == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    let cleanup_days = crate::scratchpad::preferences::load_preferences(&tx)?.auto_cleanup_days;
    let unified_id = upsert_dock_projection_in_transaction(&tx, id, cleanup_days)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    Ok(updated_mutation(revision, unified_id))
}

fn updated_mutation(revision: i64, unified_id: String) -> ContentMutation<()> {
    ContentMutation {
        value: (),
        revision,
        changes: vec![ContentChange {
            id: unified_id,
            operation: ContentOperation::Updated,
        }],
    }
}

pub fn toggle_collapse(conn: &mut Connection, id: &str, collapsed: bool) -> StorageResult<()> {
    let now = now_rfc3339();
    let rows = conn.execute(
        "UPDATE entries SET collapsed = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, collapsed as i32, now],
    )?;
    if rows == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    Ok(())
}

pub fn rename_entry(conn: &mut Connection, id: &str, title: Option<&str>) -> StorageResult<()> {
    if content_schema_exists(conn)? {
        rename_entry_with_revision(conn, id, title)?;
        return Ok(());
    }

    let now = now_rfc3339();
    let rows = conn.execute(
        "UPDATE entries SET title = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, title, now],
    )?;
    if rows == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    Ok(())
}

pub fn rename_entry_with_revision(
    conn: &mut Connection,
    id: &str,
    title: Option<&str>,
) -> StorageResult<ContentMutation<()>> {
    let now = now_rfc3339();
    let tx = conn.transaction()?;
    let rows = tx.execute(
        "UPDATE entries SET title = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, title, now],
    )?;
    if rows == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    let cleanup_days = crate::scratchpad::preferences::load_preferences(&tx)?.auto_cleanup_days;
    let unified_id = upsert_dock_projection_in_transaction(&tx, id, cleanup_days)?;
    let revision = bump_revision(&tx)?;
    tx.commit()?;
    Ok(updated_mutation(revision, unified_id))
}

pub fn create_text_item(
    conn: &mut Connection,
    content: &str,
    source: &str,
) -> StorageResult<ScratchpadItem> {
    let id = format!(
        "sp-{}",
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let now = Utc::now().to_rfc3339();
    let item = ScratchpadItem {
        id: id.clone(),
        item_type: "text".to_string(),
        content: Some(content.to_string()),
        file_path: None,
        file_name: None,
        mime_type: None,
        width: None,
        height: None,
        size_bytes: None,
        pinned: false,
        source: source.to_string(),
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    conn.execute(
        "INSERT INTO scratchpad_items (
            id, item_type, content, file_path, file_name, mime_type,
            width, height, size_bytes, pinned, source, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            item.id,
            item.item_type,
            item.content,
            item.file_path,
            item.file_name,
            item.mime_type,
            item.width,
            item.height,
            item.size_bytes,
            item.pinned as i32,
            item.source,
            item.created_at,
            item.updated_at,
        ],
    )?;

    Ok(item)
}

pub fn list_items(conn: &Connection) -> StorageResult<Vec<ScratchpadItem>> {
    let mut stmt =
        conn.prepare("SELECT * FROM scratchpad_items ORDER BY pinned DESC, created_at DESC")?;
    let items = stmt
        .query_map([], row_to_item)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(items)
}

pub fn update_text_content(conn: &mut Connection, id: &str, content: &str) -> StorageResult<()> {
    let now = Utc::now().to_rfc3339();
    let rows = conn.execute(
        "UPDATE scratchpad_items SET content = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, content, now],
    )?;
    if rows == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    Ok(())
}

pub fn toggle_pin(conn: &mut Connection, id: &str) -> StorageResult<bool> {
    let current: bool = conn.query_row(
        "SELECT pinned FROM scratchpad_items WHERE id = ?1",
        params![id],
        |row| Ok(row.get::<_, i32>(0)? != 0),
    )?;
    let new_pinned = !current;
    let now = Utc::now().to_rfc3339();
    let rows = conn.execute(
        "UPDATE scratchpad_items SET pinned = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, new_pinned as i32, now],
    )?;
    if rows == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    Ok(new_pinned)
}

pub fn delete_item(conn: &mut Connection, id: &str) -> StorageResult<()> {
    let rows = conn.execute("DELETE FROM scratchpad_items WHERE id = ?1", params![id])?;
    if rows == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    Ok(())
}

pub fn clear_unpinned(conn: &mut Connection) -> StorageResult<usize> {
    let rows = conn.execute("DELETE FROM scratchpad_items WHERE pinned = 0", [])?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn seed_legacy_v1(conn: &mut Connection) {
        conn.execute_batch(
            r#"
            CREATE TABLE schema_version (
                scope TEXT PRIMARY KEY CHECK (scope = 'main'),
                version INTEGER NOT NULL
            );

            INSERT INTO schema_version(scope, version)
            VALUES ('main', 0);

            CREATE TABLE scratchpad_items (
                id TEXT PRIMARY KEY,
                item_type TEXT NOT NULL,
                content TEXT,
                file_path TEXT,
                file_name TEXT,
                mime_type TEXT,
                width INTEGER,
                height INTEGER,
                size_bytes INTEGER,
                pinned INTEGER NOT NULL DEFAULT 0,
                source TEXT NOT NULL DEFAULT 'manual',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            INSERT INTO scratchpad_items (
                id, item_type, content, file_path, file_name, mime_type,
                width, height, size_bytes, pinned, source, created_at, updated_at
            ) VALUES (
                'sp-legacy-1', 'text', 'legacy text', NULL, NULL, NULL,
                NULL, NULL, NULL, 1, 'manual',
                '2026-04-19T00:00:00Z', '2026-04-19T00:00:00Z'
            );
            "#,
        )
        .unwrap();
    }

    fn insert_test_text_entry(
        conn: &mut Connection,
        id: &str,
        view: EntryView,
        created_at: &str,
        content: &str,
    ) {
        conn.execute(
            "INSERT INTO entries (
                id, kind, content, file_path, file_name, mime_type,
                width, height, size_bytes, collapsed, source, created_at, updated_at
            ) VALUES (?1, ?2, ?3, NULL, NULL, NULL, NULL, NULL, NULL, 0, 'manual', ?4, ?4)",
            params![id, EntryKind::Text.as_str(), content, created_at],
        )
        .unwrap();

        let table = view.membership_table();
        conn.execute(
            &format!("INSERT INTO {table} (entry_id, created_at) VALUES (?1, ?2)"),
            params![id, created_at],
        )
        .unwrap();
    }

    fn insert_test_image_entry(conn: &mut Connection, id: &str, view: EntryView, created_at: &str) {
        conn.execute(
            "INSERT INTO entries (
                id, kind, content, file_path, file_name, mime_type,
                width, height, size_bytes, collapsed, source, created_at, updated_at
            ) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, 0, 'manual', ?9, ?9)",
            params![
                id,
                EntryKind::Image.as_str(),
                "/tmp/image.png",
                "image.png",
                "image/png",
                640_i64,
                480_i64,
                1234_i64,
                created_at,
            ],
        )
        .unwrap();

        let table = view.membership_table();
        conn.execute(
            &format!("INSERT INTO {table} (entry_id, created_at) VALUES (?1, ?2)"),
            params![id, created_at],
        )
        .unwrap();
    }

    fn ensure_unified_schema(conn: &mut Connection) {
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        ensure_dock_schema(conn).unwrap();
        crate::vault::storage::ensure_vault_schema(conn).unwrap();
        crate::content::migrations::ensure_content_schema(conn, 7).unwrap();
    }

    fn assert_unified_rows(conn: &Connection, unified_id: &str, expected: i64) {
        for table in ["content_catalog", "content_fts"] {
            assert_eq!(
                conn.query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE unified_id=?1"),
                    params![unified_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
                expected,
                "{table} row count for {unified_id}"
            );
        }
    }

    fn assert_dock_projection_matches(conn: &Connection, unified_id: &str) {
        assert_unified_rows(conn, unified_id, 1);
    }

    #[test]
    fn legacy_dock_writes_cannot_bypass_unified_projection() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_unified_schema(&mut conn);
        crate::scratchpad::preferences::save_preferences(
            &mut conn,
            &crate::models::preferences::DockPreferences {
                auto_cleanup_days: 7,
                ..Default::default()
            },
        )
        .unwrap();

        let created =
            create_text_entry_with_revision(&mut conn, EntryView::Home, "project me", "manual")
                .unwrap();
        assert_eq!(created.revision, 1);
        assert_eq!(created.changes.len(), 1);
        assert_eq!(created.changes[0].operation, ContentOperation::Created);
        let entry = created.value;
        let unified_id = format!("dock:{}", entry.id);

        let initial_retention: (String, String, Option<String>, Option<f64>, Option<f64>) = conn
            .query_row(
                "SELECT retention_state, retention_changed_at, cleanup_at,
                        inbox_position, saved_position
                 FROM content_catalog WHERE unified_id=?1",
                params![unified_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(initial_retention.0, "temporary");
        let changed_at = DateTime::parse_from_rfc3339(&initial_retention.1).unwrap();
        let cleanup_at =
            DateTime::parse_from_rfc3339(initial_retention.2.as_deref().unwrap()).unwrap();
        assert_eq!(cleanup_at - changed_at, chrono::Duration::days(7));
        assert!(initial_retention.3.is_some());
        assert!(initial_retention.4.is_none());
        assert_dock_projection_matches(&conn, &unified_id);

        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM content_catalog WHERE unified_id=?1",
                params![unified_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1,
            "legacy Dock creation must populate content_catalog"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1,
            "legacy Dock creation must populate content_fts"
        );

        let updated =
            update_entry_text_with_revision(&mut conn, &entry.id, "updated projection body")
                .unwrap();
        assert_eq!(updated.revision, 2);
        assert_eq!(updated.changes[0].operation, ContentOperation::Updated);
        assert_dock_projection_matches(&conn, &unified_id);
        assert_eq!(
            conn.query_row(
                "SELECT body FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "updated projection body",
            "text updates must replace the FTS body before returning"
        );
        assert_eq!(
            conn.query_row(
                "SELECT revision FROM content_state WHERE singleton=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            2
        );

        let renamed =
            rename_entry_with_revision(&mut conn, &entry.id, Some("Projected title")).unwrap();
        assert_eq!(renamed.revision, 3);
        assert_eq!(renamed.changes[0].operation, ContentOperation::Updated);
        assert_dock_projection_matches(&conn, &unified_id);
        let retention_after_payload_updates: (
            String,
            String,
            Option<String>,
            Option<f64>,
            Option<f64>,
        ) = conn
            .query_row(
                "SELECT retention_state, retention_changed_at, cleanup_at,
                        inbox_position, saved_position
                 FROM content_catalog WHERE unified_id=?1",
                params![unified_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(retention_after_payload_updates, initial_retention);
        assert_eq!(
            conn.query_row(
                "SELECT title FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "Projected title"
        );
        assert_eq!(
            conn.query_row(
                "SELECT revision FROM content_state WHERE singleton=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            3
        );

        let saved_mutation = add_to_note_with_revision(&mut conn, &entry.id).unwrap();
        assert_eq!(saved_mutation.revision, 4);
        assert_eq!(
            saved_mutation.changes[0].operation,
            ContentOperation::Retention
        );
        assert_dock_projection_matches(&conn, &unified_id);
        let saved: (String, Option<String>, Option<f64>) = conn
            .query_row(
                "SELECT retention_state, cleanup_at, saved_position
                 FROM content_catalog WHERE unified_id=?1",
                params![unified_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(saved.0, "saved");
        assert!(saved.1.is_none());
        assert!(saved.2.is_some());

        let removed_home =
            remove_from_view_with_revision(&mut conn, EntryView::Home, &entry.id).unwrap();
        assert_eq!(removed_home.revision, 5);
        assert_eq!(
            removed_home.changes[0].operation,
            ContentOperation::Retention
        );
        assert_dock_projection_matches(&conn, &unified_id);
        for (sql, id) in [
            (
                "SELECT COUNT(*) FROM entries WHERE id=?1",
                entry.id.as_str(),
            ),
            (
                "SELECT COUNT(*) FROM note_entries WHERE entry_id=?1",
                entry.id.as_str(),
            ),
            (
                "SELECT COUNT(*) FROM content_catalog WHERE unified_id=?1",
                unified_id.as_str(),
            ),
            (
                "SELECT COUNT(*) FROM content_fts WHERE unified_id=?1",
                unified_id.as_str(),
            ),
        ] {
            assert_eq!(
                conn.query_row(sql, params![id], |row| row.get::<_, i64>(0))
                    .unwrap(),
                1,
                "removing Home from saved content must preserve {sql}"
            );
        }

        let unsaved =
            remove_from_view_with_revision(&mut conn, EntryView::Note, &entry.id).unwrap();
        assert_eq!(unsaved.revision, 6);
        assert_eq!(unsaved.changes[0].operation, ContentOperation::Retention);
        assert_dock_projection_matches(&conn, &unified_id);
        let temporary: (String, String, Option<String>, Option<f64>) = conn
            .query_row(
                "SELECT retention_state, retention_changed_at, cleanup_at, inbox_position
                 FROM content_catalog WHERE unified_id=?1",
                params![unified_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(temporary.0, "temporary");
        let changed_at = DateTime::parse_from_rfc3339(&temporary.1).unwrap();
        let cleanup_at = DateTime::parse_from_rfc3339(temporary.2.as_deref().unwrap()).unwrap();
        assert_eq!(cleanup_at - changed_at, chrono::Duration::days(7));
        assert!(temporary.3.is_some());

        let text_file_sentinel = std::env::temp_dir().join(format!(
            "dock-text-file-sentinel-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::write(&text_file_sentinel, b"text rows do not own attachments").unwrap();
        conn.execute(
            "UPDATE entries SET file_path=?2 WHERE id=?1",
            params![entry.id, text_file_sentinel.to_string_lossy().to_string()],
        )
        .unwrap();
        let deleted =
            remove_from_view_with_revision(&mut conn, EntryView::Home, &entry.id).unwrap();
        assert_eq!(deleted.revision, 7);
        assert_eq!(deleted.changes[0].operation, ContentOperation::Deleted);
        assert_unified_rows(&conn, &unified_id, 0);
        assert!(text_file_sentinel.exists());
        std::fs::remove_file(text_file_sentinel).unwrap();
        for (sql, id) in [
            (
                "SELECT COUNT(*) FROM entries WHERE id=?1",
                entry.id.as_str(),
            ),
            (
                "SELECT COUNT(*) FROM content_catalog WHERE unified_id=?1",
                unified_id.as_str(),
            ),
            (
                "SELECT COUNT(*) FROM content_fts WHERE unified_id=?1",
                unified_id.as_str(),
            ),
        ] {
            assert_eq!(
                conn.query_row(sql, params![id], |row| row.get::<_, i64>(0))
                    .unwrap(),
                0,
                "removing the final temporary membership must delete {sql}"
            );
        }
    }

    #[test]
    fn dock_reorder_updates_only_dock_scope_and_rejects_invalid_sets_atomically() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_unified_schema(&mut conn);
        let first = create_text_entry(&mut conn, EntryView::Home, "first", "manual").unwrap();
        let second = create_text_entry(&mut conn, EntryView::Home, "second", "manual").unwrap();
        let third = create_text_entry(&mut conn, EntryView::Home, "third", "manual").unwrap();
        conn.execute(
            "INSERT INTO content_catalog(
                 unified_id, source, source_id, kind, retention_state,
                 retention_changed_at, cleanup_at, inbox_position, saved_position,
                 created_at, updated_at
             ) VALUES (
                 'vault:outside-dock', 'vault', 'outside-dock', 'note', 'saved',
                 '2026-07-18T08:00:00Z', NULL, NULL, 50.0,
                 '2026-07-18T08:00:00Z', '2026-07-18T08:00:00Z'
             )",
            [],
        )
        .unwrap();
        let before_revision = crate::content::catalog::current_revision(&conn).unwrap();
        let ordered = vec![first.id.clone(), third.id.clone(), second.id.clone()];

        let mutation = reorder_entries_with_revision(&mut conn, EntryView::Home, &ordered).unwrap();
        assert_eq!(mutation.revision, before_revision + 1);
        assert_eq!(
            mutation
                .changes
                .iter()
                .map(|change| (change.id.clone(), change.operation))
                .collect::<Vec<_>>(),
            vec![
                (format!("dock:{}", first.id), ContentOperation::Reordered,),
                (format!("dock:{}", third.id), ContentOperation::Reordered,),
                (format!("dock:{}", second.id), ContentOperation::Reordered,),
            ]
        );

        for (index, id) in ordered.iter().enumerate() {
            let positions: (f64, f64) = conn
                .query_row(
                    "SELECT h.sort_order, c.inbox_position
                     FROM home_entries h
                     JOIN content_catalog c
                       ON c.source='dock' AND c.source_id=h.entry_id
                     WHERE h.entry_id=?1",
                    params![id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(positions, (index as f64, index as f64));
            assert_dock_projection_matches(&conn, &format!("dock:{id}"));
        }
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            before_revision + 1
        );

        let revision = crate::content::catalog::current_revision(&conn).unwrap();
        let invalid = vec![first.id.clone(), first.id.clone(), second.id.clone()];
        assert!(reorder_entries(&mut conn, EntryView::Home, &invalid).is_err());
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            revision
        );
    }

    #[test]
    fn new_note_entry_starts_saved_with_one_projection_and_a_real_top_position() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_unified_schema(&mut conn);
        let first =
            create_text_entry_with_revision(&mut conn, EntryView::Note, "first saved", "manual")
                .unwrap();
        let second =
            create_text_entry_with_revision(&mut conn, EntryView::Note, "second saved", "manual")
                .unwrap();
        let first_id = format!("dock:{}", first.value.id);
        let second_id = format!("dock:{}", second.value.id);

        let first_state: (String, Option<String>, Option<f64>) = conn
            .query_row(
                "SELECT retention_state, cleanup_at, saved_position
                 FROM content_catalog WHERE unified_id=?1",
                params![first_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let second_state: (String, Option<String>, Option<f64>) = conn
            .query_row(
                "SELECT retention_state, cleanup_at, saved_position
                 FROM content_catalog WHERE unified_id=?1",
                params![second_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(first_state.0, "saved");
        assert_eq!(second_state.0, "saved");
        assert!(first_state.1.is_none());
        assert!(second_state.1.is_none());
        assert!(second_state.2.unwrap() < first_state.2.unwrap());
        assert_dock_projection_matches(&conn, &first_id);
        assert_dock_projection_matches(&conn, &second_id);
    }

    #[test]
    fn catalog_and_projection_failures_roll_back_legacy_payload_and_revision() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_unified_schema(&mut conn);
        conn.execute_batch(
            "CREATE TRIGGER fail_dock_catalog_insert
             BEFORE INSERT ON content_catalog
             WHEN NEW.source='dock'
             BEGIN SELECT RAISE(ABORT, 'forced catalog failure'); END;",
        )
        .unwrap();

        assert!(create_text_entry_with_revision(
            &mut conn,
            EntryView::Home,
            "must roll back",
            "manual",
        )
        .is_err());
        for table in ["entries", "home_entries", "content_catalog", "content_fts"] {
            assert_eq!(
                conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
                0,
                "{table}"
            );
        }
        assert_eq!(crate::content::catalog::current_revision(&conn).unwrap(), 0);
        conn.execute_batch("DROP TRIGGER fail_dock_catalog_insert;")
            .unwrap();

        let entry = create_text_entry(&mut conn, EntryView::Home, "old body", "manual").unwrap();
        let unified_id = format!("dock:{}", entry.id);
        let old_catalog_updated_at: String = conn
            .query_row(
                "SELECT updated_at FROM content_catalog WHERE unified_id=?1",
                params![unified_id],
                |row| row.get(0),
            )
            .unwrap();
        let revision = crate::content::catalog::current_revision(&conn).unwrap();
        conn.execute_batch(
            "DROP TABLE content_fts;
             CREATE TABLE content_fts(
                 unified_id TEXT NOT NULL,
                 title TEXT NOT NULL,
                 body TEXT NOT NULL CHECK(body <> 'blocked body'),
                 tags TEXT NOT NULL,
                 aliases TEXT NOT NULL
             );
             INSERT INTO content_fts(unified_id, title, body, tags, aliases)
             SELECT 'placeholder', '', '', '', '';
             DELETE FROM content_fts;",
        )
        .unwrap();
        let old_document = build_search_document(&conn, &unified_id).unwrap();
        replace_projection(&conn, &old_document).unwrap();

        assert!(update_entry_text_with_revision(&mut conn, &entry.id, "blocked body").is_err());
        assert_eq!(
            conn.query_row(
                "SELECT content FROM entries WHERE id=?1",
                params![entry.id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "old body"
        );
        assert_eq!(
            conn.query_row(
                "SELECT body FROM content_fts WHERE unified_id=?1",
                params![unified_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "old body"
        );
        assert_eq!(
            conn.query_row(
                "SELECT updated_at FROM content_catalog WHERE unified_id=?1",
                params![unified_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            old_catalog_updated_at
        );
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            revision
        );
    }

    #[test]
    fn retention_transition_failures_roll_back_membership_catalog_and_revision() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_unified_schema(&mut conn);
        let entry = create_text_entry(&mut conn, EntryView::Home, "retention", "manual").unwrap();
        let unified_id = format!("dock:{}", entry.id);
        let revision = crate::content::catalog::current_revision(&conn).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_note_membership
             BEFORE INSERT ON note_entries
             BEGIN SELECT RAISE(ABORT, 'forced note failure'); END;",
        )
        .unwrap();

        assert!(add_to_note_with_revision(&mut conn, &entry.id).is_err());
        assert_eq!(
            conn.query_row(
                "SELECT retention_state FROM content_catalog WHERE unified_id=?1",
                params![unified_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "temporary"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM note_entries WHERE entry_id=?1",
                params![entry.id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            revision
        );

        conn.execute_batch("DROP TRIGGER fail_note_membership;")
            .unwrap();
        add_to_note_with_revision(&mut conn, &entry.id).unwrap();
        let revision = crate::content::catalog::current_revision(&conn).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_home_membership
             BEFORE INSERT ON home_entries
             BEGIN SELECT RAISE(ABORT, 'forced Home failure'); END;",
        )
        .unwrap();

        assert!(remove_from_view_with_revision(&mut conn, EntryView::Note, &entry.id).is_err());
        assert_eq!(
            conn.query_row(
                "SELECT retention_state FROM content_catalog WHERE unified_id=?1",
                params![unified_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "saved"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM note_entries WHERE entry_id=?1",
                params![entry.id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            revision
        );
    }

    #[test]
    fn reorder_database_failure_rolls_back_both_order_columns_and_revision() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_unified_schema(&mut conn);
        let first = create_text_entry(&mut conn, EntryView::Home, "first", "manual").unwrap();
        let second = create_text_entry(&mut conn, EntryView::Home, "second", "manual").unwrap();
        let before: Vec<(String, f64, f64)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT h.entry_id, h.sort_order, c.inbox_position
                     FROM home_entries h
                     JOIN content_catalog c ON c.source_id=h.entry_id
                     ORDER BY h.entry_id",
                )
                .unwrap();
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        let revision = crate::content::catalog::current_revision(&conn).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_legacy_reorder
             BEFORE UPDATE OF sort_order ON home_entries
             BEGIN SELECT RAISE(ABORT, 'forced reorder failure'); END;",
        )
        .unwrap();

        assert!(
            reorder_entries_with_revision(&mut conn, EntryView::Home, &[first.id, second.id],)
                .is_err()
        );
        let after: Vec<(String, f64, f64)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT h.entry_id, h.sort_order, c.inbox_position
                     FROM home_entries h
                     JOIN content_catalog c ON c.source_id=h.entry_id
                     ORDER BY h.entry_id",
                )
                .unwrap();
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(after, before);
        assert_eq!(
            crate::content::catalog::current_revision(&conn).unwrap(),
            revision
        );
    }

    #[test]
    fn migrates_legacy_rows_into_entries_and_memberships() {
        let mut conn = Connection::open_in_memory().unwrap();
        seed_legacy_v1(&mut conn);

        ensure_dock_schema(&mut conn).unwrap();

        let entries = list_entries(&conn, EntryView::Home, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "sp-legacy-1");
        assert!(entries[0].collapsed);
        assert!(entries[0].in_home);
        assert!(entries[0].in_note);

        let home_entries: i64 = conn
            .query_row("SELECT COUNT(*) FROM home_entries", [], |row| row.get(0))
            .unwrap();
        let note_entries: i64 = conn
            .query_row("SELECT COUNT(*) FROM note_entries", [], |row| row.get(0))
            .unwrap();

        assert_eq!(home_entries, 1);
        assert_eq!(note_entries, 1);
    }

    #[test]
    fn list_entries_filters_by_kind_and_orders_by_newest_membership_first() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();

        insert_test_text_entry(
            &mut conn,
            "de-old-text",
            EntryView::Home,
            "2026-04-18T00:00:00Z",
            "older text",
        );
        insert_test_image_entry(
            &mut conn,
            "de-new-image",
            EntryView::Home,
            "2026-04-19T00:00:00Z",
        );

        let entries = list_entries(&conn, EntryView::Home, None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "de-new-image");
        assert_eq!(entries[1].id, "de-old-text");

        let images = list_entries(&conn, EntryView::Home, Some(EntryKind::Image)).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].id, "de-new-image");
        assert_eq!(images[0].kind, EntryKind::Image);
    }

    #[test]
    fn removing_home_membership_keeps_entry_alive_in_note() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();

        let entry =
            create_text_entry(&mut conn, EntryView::Home, "shared note text", "manual").unwrap();
        add_to_note(&mut conn, &entry.id).unwrap();
        add_to_note(&mut conn, &entry.id).unwrap();

        remove_from_view(&mut conn, EntryView::Home, &entry.id).unwrap();

        let home_entries = list_entries(&conn, EntryView::Home, None).unwrap();
        let note_entries = list_entries(&conn, EntryView::Note, None).unwrap();

        assert!(home_entries.is_empty());
        assert_eq!(note_entries.len(), 1);
        assert_eq!(note_entries[0].id, entry.id);
        assert!(!note_entries[0].in_home);
        assert!(note_entries[0].in_note);

        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn cleanup_home_on_startup_deletes_home_only_entries() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();

        let _home_only =
            create_text_entry(&mut conn, EntryView::Home, "remove me", "manual").unwrap();
        let shared = create_text_entry(&mut conn, EntryView::Home, "keep me", "manual").unwrap();
        add_to_note(&mut conn, &shared.id).unwrap();

        let deleted = cleanup_home_on_startup(&mut conn, 0).unwrap();
        assert_eq!(deleted, 1);

        let home_entries = list_entries(&conn, EntryView::Home, None).unwrap();
        let note_entries = list_entries(&conn, EntryView::Note, None).unwrap();

        assert_eq!(home_entries.len(), 1);
        assert_eq!(home_entries[0].id, shared.id);
        assert_eq!(note_entries.len(), 1);
        assert_eq!(note_entries[0].id, shared.id);

        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn toggle_collapse_returns_error_for_missing_entry() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();

        let result = toggle_collapse(&mut conn, "missing-entry", true);

        assert!(result.is_err());
    }

    #[test]
    fn rename_entry_returns_error_for_missing_entry() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();

        let result = rename_entry(&mut conn, "missing-entry", Some("title"));

        assert!(result.is_err());
    }

    #[test]
    fn removing_last_view_deletes_associated_file_from_disk() {
        let dir = std::env::temp_dir().join("scratchpad_test_file_cleanup");
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("test-image.png");
        std::fs::write(&file_path, b"fake png content").unwrap();
        assert!(file_path.exists());

        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();

        let path_str = file_path.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO entries (
                id, kind, content, file_path, file_name, mime_type,
                width, height, size_bytes, collapsed, source, created_at, updated_at
            ) VALUES ('de-file-1', 'image', NULL, ?1, 'test-image.png', 'image/png',
                       640, 480, 1024, 0, 'manual', '2026-04-26T00:00:00Z', '2026-04-26T00:00:00Z')",
            params![path_str],
        ).unwrap();
        conn.execute(
            "INSERT INTO home_entries (entry_id, created_at) VALUES ('de-file-1', '2026-04-26T00:00:00Z')",
            [],
        ).unwrap();

        remove_from_view(&mut conn, EntryView::Home, "de-file-1").unwrap();

        // Entry should be gone from DB
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0);

        // File should be gone from disk
        assert!(
            !file_path.exists(),
            "file should have been deleted from disk"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn legacy_fallback_does_not_delete_file_path_owned_by_text_content() {
        let sentinel = std::env::temp_dir().join(format!(
            "legacy-text-file-sentinel-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::write(&sentinel, b"not an attachment").unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO entries(
                 id, kind, content, file_path, collapsed, source, created_at, updated_at
             ) VALUES (
                 'legacy-text-file', 'text', 'text', ?1, 0, 'manual',
                 '2026-07-18T08:00:00Z', '2026-07-18T08:00:00Z'
             )",
            params![sentinel.to_string_lossy().to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO home_entries(entry_id, created_at, sort_order)
             VALUES ('legacy-text-file', '2026-07-18T08:00:00Z', 0.0)",
            [],
        )
        .unwrap();

        remove_from_view(&mut conn, EntryView::Home, "legacy-text-file").unwrap();

        assert!(sentinel.exists());
        std::fs::remove_file(sentinel).unwrap();
    }

    #[test]
    fn legacy_fallback_database_failure_keeps_attachment_until_commit() {
        let attachment = std::env::temp_dir().join(format!(
            "legacy-image-rollback-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::write(&attachment, b"attachment").unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO entries(
                 id, kind, file_path, collapsed, source, created_at, updated_at
             ) VALUES (
                 'legacy-image-rollback', 'image', ?1, 0, 'manual',
                 '2026-07-18T08:00:00Z', '2026-07-18T08:00:00Z'
             )",
            params![attachment.to_string_lossy().to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO home_entries(entry_id, created_at, sort_order)
             VALUES ('legacy-image-rollback', '2026-07-18T08:00:00Z', 0.0)",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_legacy_attachment_delete
             BEFORE DELETE ON entries
             BEGIN SELECT RAISE(ABORT, 'forced payload delete failure'); END;",
        )
        .unwrap();

        assert!(remove_from_view(&mut conn, EntryView::Home, "legacy-image-rollback").is_err());

        assert!(attachment.exists());
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM home_entries WHERE entry_id='legacy-image-rollback'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        std::fs::remove_file(attachment).unwrap();
    }

    #[test]
    fn keeping_entry_in_another_view_preserves_file_on_disk() {
        let dir = std::env::temp_dir().join("scratchpad_test_file_preserve");
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("shared-doc.pdf");
        std::fs::write(&file_path, b"fake pdf content").unwrap();

        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();

        let path_str = file_path.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO entries (
                id, kind, content, file_path, file_name, mime_type,
                width, height, size_bytes, collapsed, source, created_at, updated_at
            ) VALUES ('de-file-2', 'file', NULL, ?1, 'shared-doc.pdf', 'application/pdf',
                       NULL, NULL, 2048, 0, 'manual', '2026-04-26T00:00:00Z', '2026-04-26T00:00:00Z')",
            params![path_str],
        ).unwrap();
        conn.execute(
            "INSERT INTO home_entries (entry_id, created_at) VALUES ('de-file-2', '2026-04-26T00:00:00Z')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO note_entries (entry_id, created_at) VALUES ('de-file-2', '2026-04-26T00:00:00Z')",
            [],
        ).unwrap();

        // Remove from home only — entry still in note, file must survive
        remove_from_view(&mut conn, EntryView::Home, "de-file-2").unwrap();

        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1, "entry should still exist in DB");

        assert!(file_path.exists(), "file should still exist on disk");

        // Now remove from note too — file should be cleaned up
        remove_from_view(&mut conn, EntryView::Note, "de-file-2").unwrap();

        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0, "entry should be fully removed");

        assert!(
            !file_path.exists(),
            "file should be deleted after last view removed"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn text_entry_deletion_works_without_file() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();

        let entry = create_text_entry(&mut conn, EntryView::Home, "hello", "manual").unwrap();
        remove_from_view(&mut conn, EntryView::Home, &entry.id).unwrap();

        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0, "text entry should be removed without error");
    }

    #[test]
    fn cleanup_with_days_preserves_recent_entries() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();

        // Insert a home-only entry created 2 days ago
        conn.execute(
            "INSERT INTO entries (id, kind, content, collapsed, source, created_at, updated_at)
             VALUES ('de-old', 'text', 'old', 0, 'manual', datetime('now', '-2 days'), datetime('now', '-2 days'))",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO home_entries (entry_id, created_at) VALUES ('de-old', datetime('now', '-2 days'))",
            [],
        ).unwrap();

        // Insert a home-only entry created just now
        let recent = create_text_entry(&mut conn, EntryView::Home, "recent", "manual").unwrap();

        // With max_age_days=1, only the 2-day-old entry should be deleted
        let deleted = cleanup_home_on_startup(&mut conn, 1).unwrap();
        assert_eq!(deleted, 1);

        let home = list_entries(&conn, EntryView::Home, None).unwrap();
        assert_eq!(home.len(), 1);
        assert_eq!(home[0].id, recent.id);
    }

    #[test]
    fn cleanup_with_zero_days_deletes_all_unstarred() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();

        let _e1 = create_text_entry(&mut conn, EntryView::Home, "a", "manual").unwrap();
        let _e2 = create_text_entry(&mut conn, EntryView::Home, "b", "manual").unwrap();

        let deleted = cleanup_home_on_startup(&mut conn, 0).unwrap();
        assert_eq!(deleted, 2);

        let home = list_entries(&conn, EntryView::Home, None).unwrap();
        assert!(home.is_empty());
    }

    /// End-to-end: simulate app startup with auto_cleanup_days from preferences
    #[test]
    fn e2e_cleanup_driven_by_preferences() {
        use crate::models::preferences::DockPreferences;
        use crate::scratchpad::preferences::{load_preferences, save_preferences};

        let mut conn = Connection::open_in_memory().unwrap();

        // Phase 1: First startup with default prefs (auto_cleanup_days = 0)
        ensure_dock_schema(&mut conn).unwrap();
        let _home_only =
            create_text_entry(&mut conn, EntryView::Home, "will be cleaned", "manual").unwrap();
        let starred =
            create_text_entry(&mut conn, EntryView::Home, "keep forever", "manual").unwrap();
        add_to_note(&mut conn, &starred.id).unwrap();

        // Simulate app reading prefs and running cleanup
        let prefs = load_preferences(&conn).unwrap();
        assert_eq!(prefs.auto_cleanup_days, 0);

        let deleted = cleanup_home_on_startup(&mut conn, prefs.auto_cleanup_days).unwrap();
        assert_eq!(deleted, 1, "one unstarred entry should be cleaned");

        let home = list_entries(&conn, EntryView::Home, None).unwrap();
        let note = list_entries(&conn, EntryView::Note, None).unwrap();
        assert_eq!(home.len(), 1, "starred entry still in home");
        assert_eq!(note.len(), 1, "starred entry in note");

        // Phase 2: User changes auto_cleanup_days to 7
        let new_prefs = DockPreferences {
            auto_cleanup_days: 7,
            ..Default::default()
        };
        save_preferences(&mut conn, &new_prefs).unwrap();

        // Insert entries of different ages
        conn.execute(
            "INSERT INTO entries (id, kind, content, collapsed, source, created_at, updated_at)
             VALUES ('de-1d', 'text', '1 day old', 0, 'manual', datetime('now', '-1 day'), datetime('now', '-1 day'))",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO home_entries (entry_id, created_at) VALUES ('de-1d', datetime('now', '-1 day'))",
            [],
        ).unwrap();

        conn.execute(
            "INSERT INTO entries (id, kind, content, collapsed, source, created_at, updated_at)
             VALUES ('de-10d', 'text', '10 days old', 0, 'manual', datetime('now', '-10 days'), datetime('now', '-10 days'))",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO home_entries (entry_id, created_at) VALUES ('de-10d', datetime('now', '-10 days'))",
            [],
        ).unwrap();

        // Reload prefs and cleanup
        let prefs2 = load_preferences(&conn).unwrap();
        assert_eq!(prefs2.auto_cleanup_days, 7);

        let deleted2 = cleanup_home_on_startup(&mut conn, prefs2.auto_cleanup_days).unwrap();
        assert_eq!(
            deleted2, 1,
            "only the 10-day-old unstarred entry should be cleaned"
        );

        let home2 = list_entries(&conn, EntryView::Home, None).unwrap();
        assert!(
            home2.iter().any(|e| e.id == "de-1d"),
            "1-day-old entry survives"
        );
        assert!(
            home2.iter().any(|e| e.id == starred.id),
            "starred entry survives"
        );
        assert!(
            !home2.iter().any(|e| e.id == "de-10d"),
            "10-day-old entry was cleaned"
        );
    }
}
