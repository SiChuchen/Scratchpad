use std::fs;
use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, Row};

use crate::models::entry::{DockEntry, EntryKind, EntryView};
use crate::models::scratchpad::ScratchpadItem;
use crate::storage::error::StorageResult;
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
        format!(
            "INSERT OR IGNORE INTO {table} (entry_id, created_at) VALUES (?1, ?2)"
        )
    } else {
        format!("INSERT INTO {table} (entry_id, created_at) VALUES (?1, ?2)")
    };
    conn.execute(&sql, params![entry_id, created_at])?;
    Ok(())
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

fn delete_orphaned_entry_internal(conn: &Connection, entry_id: &str) -> StorageResult<bool> {
    if membership_count(conn, entry_id)? == 0 {
        // Clean up associated file on disk before removing the DB row
        let file_path: Option<String> = conn
            .query_row(
                "SELECT file_path FROM entries WHERE id = ?1",
                params![entry_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        if let Some(ref path) = file_path {
            let _ = fs::remove_file(Path::new(path));
        }

        let rows = conn.execute("DELETE FROM entries WHERE id = ?1", params![entry_id])?;
        return Ok(rows > 0);
    }
    Ok(false)
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
        stmt.query_map(
            params![format!("-{} days", max_age_days)],
            |row| row.get(0),
        )?
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
        Migration::new(2, "add title column", "ALTER TABLE entries ADD COLUMN title TEXT"),
    ]
}

pub fn ensure_dock_schema(conn: &mut Connection, auto_cleanup_days: i64) -> StorageResult<()> {
    ensure_schema(conn, &dock_migrations())?;
    conn.execute_batch(DOCK_SCHEMA_SQL)?;
    migrate_legacy_scratchpad_items(conn)?;
    cleanup_home_on_startup(conn, auto_cleanup_days)?;
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
    if !entry_exists(conn, entry_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }

    let now = now_rfc3339();
    let tx = conn.transaction()?;
    insert_membership_row(&tx, EntryView::Note, entry_id, &now, true)?;
    tx.commit()?;
    Ok(())
}

pub fn remove_from_view(
    conn: &mut Connection,
    view: EntryView,
    entry_id: &str,
) -> StorageResult<()> {
    let tx = conn.transaction()?;
    let rows = remove_membership_row(&tx, view, entry_id)?;
    if rows == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    let _ = delete_orphaned_entry_internal(&tx, entry_id)?;
    tx.commit()?;
    Ok(())
}

pub fn reorder_entries(
    conn: &mut Connection,
    view: EntryView,
    ordered_ids: &[String],
) -> StorageResult<()> {
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

pub fn cleanup_home_on_startup(conn: &mut Connection, max_age_days: i64) -> StorageResult<usize> {
    let tx = conn.transaction()?;
    let ids = home_only_entry_ids(&tx, max_age_days)?;
    let mut deleted = 0usize;

    for entry_id in ids {
        let existed = entry_exists(&tx, &entry_id)?;
        remove_membership_row(&tx, EntryView::Home, &entry_id)?;
        if existed && delete_orphaned_entry_internal(&tx, &entry_id)? {
            deleted += 1;
        }
    }

    tx.commit()?;
    Ok(deleted)
}

pub fn delete_orphaned_entry(conn: &mut Connection, entry_id: &str) -> StorageResult<()> {
    let _ = delete_orphaned_entry_internal(conn, entry_id)?;
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

pub fn update_entry_text(conn: &mut Connection, id: &str, content: &str) -> StorageResult<()> {
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

pub fn create_text_item(
    conn: &mut Connection,
    content: &str,
    source: &str,
) -> StorageResult<ScratchpadItem> {
    let id = format!("sp-{}", Utc::now().timestamp_nanos_opt().unwrap_or_default());
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

pub fn create_image_item(
    conn: &mut Connection,
    file_path: &str,
    file_name: &str,
    mime_type: &str,
    width: Option<i64>,
    height: Option<i64>,
    size_bytes: Option<i64>,
    source: &str,
) -> StorageResult<ScratchpadItem> {
    let id = format!("sp-{}", Utc::now().timestamp_nanos_opt().unwrap_or_default());
    let now = Utc::now().to_rfc3339();
    let item = ScratchpadItem {
        id: id.clone(),
        item_type: "image".to_string(),
        content: None,
        file_path: Some(file_path.to_string()),
        file_name: Some(file_name.to_string()),
        mime_type: Some(mime_type.to_string()),
        width,
        height,
        size_bytes,
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
    let mut stmt = conn.prepare(
        "SELECT * FROM scratchpad_items ORDER BY pinned DESC, created_at DESC",
    )?;
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
mod repository_tests {
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

    fn insert_test_image_entry(
        conn: &mut Connection,
        id: &str,
        view: EntryView,
        created_at: &str,
    ) {
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

    #[test]
    fn migrates_legacy_rows_into_entries_and_memberships() {
        let mut conn = Connection::open_in_memory().unwrap();
        seed_legacy_v1(&mut conn);

        ensure_dock_schema(&mut conn, 0).unwrap();

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
        ensure_dock_schema(&mut conn, 0).unwrap();

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
        ensure_dock_schema(&mut conn, 0).unwrap();

        let entry = create_text_entry(&mut conn, EntryView::Home, "shared note text", "manual")
            .unwrap();
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
        ensure_dock_schema(&mut conn, 0).unwrap();

        let _home_only = create_text_entry(&mut conn, EntryView::Home, "remove me", "manual")
            .unwrap();
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
        ensure_dock_schema(&mut conn, 0).unwrap();

        let result = toggle_collapse(&mut conn, "missing-entry", true);

        assert!(result.is_err());
    }

    #[test]
    fn rename_entry_returns_error_for_missing_entry() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn, 0).unwrap();

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
        ensure_dock_schema(&mut conn, 0).unwrap();

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
        assert!(!file_path.exists(), "file should have been deleted from disk");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn keeping_entry_in_another_view_preserves_file_on_disk() {
        let dir = std::env::temp_dir().join("scratchpad_test_file_preserve");
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("shared-doc.pdf");
        std::fs::write(&file_path, b"fake pdf content").unwrap();

        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn, 0).unwrap();

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

        assert!(!file_path.exists(), "file should be deleted after last view removed");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn text_entry_deletion_works_without_file() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn, 0).unwrap();

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
        ensure_dock_schema(&mut conn, 0).unwrap();

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
        ensure_dock_schema(&mut conn, 0).unwrap();

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
        use crate::scratchpad::preferences::{load_preferences, save_preferences};
        use crate::models::preferences::DockPreferences;

        let mut conn = Connection::open_in_memory().unwrap();

        // Phase 1: First startup with default prefs (auto_cleanup_days = 0)
        ensure_dock_schema(&mut conn, 0).unwrap();
        let _home_only = create_text_entry(&mut conn, EntryView::Home, "will be cleaned", "manual").unwrap();
        let starred = create_text_entry(&mut conn, EntryView::Home, "keep forever", "manual").unwrap();
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
        let mut new_prefs = DockPreferences::default();
        new_prefs.auto_cleanup_days = 7;
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
        assert_eq!(deleted2, 1, "only the 10-day-old unstarred entry should be cleaned");

        let home2 = list_entries(&conn, EntryView::Home, None).unwrap();
        assert!(home2.iter().any(|e| e.id == "de-1d"), "1-day-old entry survives");
        assert!(home2.iter().any(|e| e.id == starred.id), "starred entry survives");
        assert!(!home2.iter().any(|e| e.id == "de-10d"), "10-day-old entry was cleaned");
    }
}
