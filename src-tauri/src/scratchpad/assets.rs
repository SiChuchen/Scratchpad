use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use mime_guess::from_path as guess_mime;
use rusqlite::Connection;

use crate::content::models::ContentMutation;
use crate::models::entry::{DockEntry, EntryKind, EntryView};
use crate::scratchpad::storage::create_dock_entry_internal_with_revision;
use crate::storage::connection::data_dir;
use crate::storage::error::StorageResult;

fn unique_filename(original_name: &str) -> String {
    // Sanitize: extract basename to prevent path traversal (e.g. ../../../evil)
    let mut safe_name = Path::new(original_name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled-file".to_string());
    // Guard against "." and ".." which are directory references, not filenames
    if safe_name == "." || safe_name == ".." || safe_name.is_empty() {
        safe_name = "untitled-file".to_string();
    }
    let ts = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    format!("{}-{}", ts, safe_name)
}

pub fn assets_dir() -> StorageResult<PathBuf> {
    let dated = Utc::now().format("%Y-%m-%d").to_string();
    let dir = data_dir()?.join("assets").join(dated);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn classify_kind(path: &Path, mime: Option<&str>) -> EntryKind {
    let is_image = mime.is_some_and(|m| m.starts_with("image/"))
        || guess_mime(path)
            .first_raw()
            .is_some_and(|m| m.starts_with("image/"));
    if is_image {
        EntryKind::Image
    } else {
        EntryKind::File
    }
}

pub fn import_file(
    conn: &mut Connection,
    source_path: &str,
    view: EntryView,
) -> StorageResult<DockEntry> {
    Ok(import_file_with_revision(conn, source_path, view)?.value)
}

pub fn import_file_with_revision(
    conn: &mut Connection,
    source_path: &str,
    view: EntryView,
) -> StorageResult<ContentMutation<DockEntry>> {
    let source = Path::new(source_path);
    let file_name = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "imported-file".to_string());

    let mime_type = guess_mime(source).first_raw().map(|m| m.to_string());
    let kind = classify_kind(source, mime_type.as_deref());
    let metadata = fs::metadata(source)?;
    let size_bytes = metadata.len() as i64;

    let dest_dir = assets_dir()?;
    let disk_name = unique_filename(&file_name);
    let dest_path = dest_dir.join(&disk_name);
    fs::copy(source, &dest_path)?;

    let result = create_dock_entry_internal_with_revision(
        conn,
        view,
        kind,
        None,
        Some(dest_path.to_string_lossy().to_string()),
        Some(file_name),
        mime_type,
        None,
        None,
        Some(size_bytes),
        "drop",
    );
    cleanup_failed_import(&dest_path, result)
}

pub fn import_image_bytes(
    conn: &mut Connection,
    bytes: &[u8],
    file_name: &str,
    mime_type: &str,
    width: Option<i64>,
    height: Option<i64>,
    view: EntryView,
) -> StorageResult<DockEntry> {
    Ok(
        import_image_bytes_with_revision(conn, bytes, file_name, mime_type, width, height, view)?
            .value,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn import_image_bytes_with_revision(
    conn: &mut Connection,
    bytes: &[u8],
    file_name: &str,
    mime_type: &str,
    width: Option<i64>,
    height: Option<i64>,
    view: EntryView,
) -> StorageResult<ContentMutation<DockEntry>> {
    let dest_dir = assets_dir()?;
    let disk_name = unique_filename(file_name);
    let dest_path = dest_dir.join(&disk_name);
    fs::write(&dest_path, bytes)?;
    let size_bytes = bytes.len() as i64;
    let result = create_dock_entry_internal_with_revision(
        conn,
        view,
        EntryKind::Image,
        None,
        Some(dest_path.to_string_lossy().to_string()),
        Some(file_name.to_string()),
        Some(mime_type.to_string()),
        width,
        height,
        Some(size_bytes),
        "clipboard",
    );
    cleanup_failed_import(&dest_path, result)
}

pub fn import_file_bytes(
    conn: &mut Connection,
    bytes: &[u8],
    file_name: &str,
    mime_type: Option<&str>,
    view: EntryView,
) -> StorageResult<DockEntry> {
    Ok(import_file_bytes_with_revision(conn, bytes, file_name, mime_type, view)?.value)
}

pub fn import_file_bytes_with_revision(
    conn: &mut Connection,
    bytes: &[u8],
    file_name: &str,
    mime_type: Option<&str>,
    view: EntryView,
) -> StorageResult<ContentMutation<DockEntry>> {
    let path = Path::new(file_name);
    let kind = classify_kind(path, mime_type);
    let dest_dir = assets_dir()?;
    let disk_name = unique_filename(file_name);
    let dest_path = dest_dir.join(&disk_name);
    fs::write(&dest_path, bytes)?;
    let size_bytes = bytes.len() as i64;
    let result = create_dock_entry_internal_with_revision(
        conn,
        view,
        kind,
        None,
        Some(dest_path.to_string_lossy().to_string()),
        Some(file_name.to_string()),
        mime_type.map(|m| m.to_string()),
        None,
        None,
        Some(size_bytes),
        "drop",
    );
    cleanup_failed_import(&dest_path, result)
}

fn cleanup_failed_import<T>(dest_path: &Path, result: StorageResult<T>) -> StorageResult<T> {
    if result.is_err() {
        if let Err(error) = fs::remove_file(dest_path) {
            eprintln!(
                "failed to remove imported asset after database rollback ({}): {error}",
                dest_path.display()
            );
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unified_connection() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::scratchpad::storage::ensure_dock_schema(&mut conn).unwrap();
        crate::vault::storage::ensure_vault_schema(&mut conn).unwrap();
        crate::content::migrations::ensure_content_schema(&mut conn, 7).unwrap();
        conn
    }

    fn imported_asset_count(suffix: &str) -> usize {
        assets_dir()
            .unwrap()
            .read_dir()
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(suffix))
            .count()
    }

    #[test]
    fn failed_import_removes_only_the_new_destination_file() {
        let mut conn = unified_connection();
        conn.execute_batch(
            "CREATE TRIGGER fail_import_catalog
             BEFORE INSERT ON content_catalog
             WHEN NEW.source='dock'
             BEGIN SELECT RAISE(ABORT, 'forced import catalog failure'); END;",
        )
        .unwrap();
        let suffix = format!(
            "dock-import-source-{}.bin",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let source_dir = std::env::temp_dir().join("scratchpad-import-rollback");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join(&suffix);
        fs::write(&source, b"source must survive").unwrap();
        let before = imported_asset_count(&suffix);

        assert!(import_file(
            &mut conn,
            source.to_string_lossy().as_ref(),
            EntryView::Home,
        )
        .is_err());

        assert!(
            source.exists(),
            "the original source file must not be removed"
        );
        assert_eq!(
            imported_asset_count(&suffix),
            before,
            "a failed DB/catalog/FTS transaction must not leave an imported asset"
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM entries", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(crate::content::catalog::current_revision(&conn).unwrap(), 0);

        fs::remove_file(source).ok();
        fs::remove_dir(source_dir).ok();
    }

    #[test]
    fn every_import_path_commits_catalog_projection_and_created_mutation() {
        let mut conn = unified_connection();
        let source_dir = std::env::temp_dir().join("scratchpad-import-success");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join(format!(
            "source-{}.bin",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::write(&source, b"copied import").unwrap();

        let copied = import_file_with_revision(
            &mut conn,
            source.to_string_lossy().as_ref(),
            EntryView::Home,
        )
        .unwrap();
        let image = import_image_bytes_with_revision(
            &mut conn,
            b"image bytes",
            "clipboard-projection.png",
            "image/png",
            Some(10),
            Some(20),
            EntryView::Home,
        )
        .unwrap();
        let bytes = import_file_bytes_with_revision(
            &mut conn,
            b"file bytes",
            "drop-projection.pdf",
            Some("application/pdf"),
            EntryView::Note,
        )
        .unwrap();

        for mutation in [&copied, &image, &bytes] {
            let unified_id = format!("dock:{}", mutation.value.id);
            assert_eq!(
                mutation.changes,
                vec![crate::content::models::ContentChange {
                    id: unified_id.clone(),
                    operation: crate::content::models::ContentOperation::Created,
                }]
            );
            for table in ["content_catalog", "content_fts"] {
                assert_eq!(
                    conn.query_row(
                        &format!("SELECT COUNT(*) FROM {table} WHERE unified_id=?1"),
                        rusqlite::params![unified_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                    1,
                    "{table}"
                );
            }
        }
        assert_eq!(
            conn.query_row(
                "SELECT retention_state FROM content_catalog WHERE unified_id=?1",
                rusqlite::params![format!("dock:{}", bytes.value.id)],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "saved"
        );

        let copied_asset = PathBuf::from(copied.value.file_path.as_deref().unwrap());
        let image_asset = PathBuf::from(image.value.file_path.as_deref().unwrap());
        let bytes_asset = PathBuf::from(bytes.value.file_path.as_deref().unwrap());
        crate::scratchpad::storage::remove_from_view(&mut conn, EntryView::Home, &copied.value.id)
            .unwrap();
        crate::scratchpad::storage::remove_from_view(&mut conn, EntryView::Home, &image.value.id)
            .unwrap();
        crate::scratchpad::storage::remove_from_view(&mut conn, EntryView::Note, &bytes.value.id)
            .unwrap();
        crate::scratchpad::storage::remove_from_view(&mut conn, EntryView::Home, &bytes.value.id)
            .unwrap();
        assert!(
            source.exists(),
            "deleting the import must preserve its source"
        );
        for asset in [copied_asset, image_asset, bytes_asset] {
            assert!(
                !asset.exists(),
                "committed imported attachment must be deleted"
            );
        }
        fs::remove_file(source).ok();
        fs::remove_dir(source_dir).ok();
    }
}
