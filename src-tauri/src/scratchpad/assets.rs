use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use mime_guess::from_path as guess_mime;
use rusqlite::Connection;

use crate::models::entry::{DockEntry, EntryKind, EntryView};
use crate::storage::error::StorageResult;
use crate::scratchpad::storage::create_dock_entry_internal;

pub fn assets_dir() -> StorageResult<PathBuf> {
    let base = dirs::data_local_dir().expect("LocalAppData must exist");
    let dated = Utc::now().format("%Y-%m-%d").to_string();
    let dir = base
        .join("Soma")
        .join("scratchpad")
        .join("assets")
        .join(dated);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn classify_kind(path: &Path, mime: Option<&str>) -> EntryKind {
    if mime.map_or(false, |m| m.starts_with("image/")) {
        EntryKind::Image
    } else if guess_mime(path)
        .first_raw()
        .map_or(false, |m| m.starts_with("image/"))
    {
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
    let source = Path::new(source_path);
    let file_name = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "imported-file".to_string());

    let mime_type = guess_mime(source).first_raw().map(|m| m.to_string());
    let kind = classify_kind(source, mime_type.as_deref());

    let dest_dir = assets_dir()?;
    let dest_path = dest_dir.join(&file_name);
    fs::copy(source, &dest_path)?;

    let metadata = fs::metadata(source)?;
    let size_bytes = metadata.len() as i64;

    create_dock_entry_internal(
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
    )
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
    let dest_dir = assets_dir()?;
    let dest_path = dest_dir.join(file_name);
    fs::write(&dest_path, bytes)?;

    let size_bytes = bytes.len() as i64;

    create_dock_entry_internal(
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
    )
}

pub fn import_file_bytes(
    conn: &mut Connection,
    bytes: &[u8],
    file_name: &str,
    mime_type: Option<&str>,
    view: EntryView,
) -> StorageResult<DockEntry> {
    let path = Path::new(file_name);
    let kind = classify_kind(path, mime_type);

    let dest_dir = assets_dir()?;
    let dest_path = dest_dir.join(file_name);
    fs::write(&dest_path, bytes)?;

    let size_bytes = bytes.len() as i64;

    create_dock_entry_internal(
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
    )
}
