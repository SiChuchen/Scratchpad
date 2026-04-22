use std::{fs, io, path::PathBuf};
use rusqlite::Connection;
use super::error::{StorageError, StorageResult};

/// Returns the data directory next to the application executable.
///
/// Layout:
///   <exe_dir>/data/
///     scratchpad.sqlite3
///     assets/YYYY-MM-DD/...
pub fn data_dir() -> StorageResult<PathBuf> {
    let exe = std::env::current_exe().map_err(|e| {
        StorageError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("failed to resolve exe path: {e}"),
        ))
    })?;
    let exe_dir = exe.parent().ok_or_else(|| {
        StorageError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "exe has no parent directory",
        ))
    })?;
    let dir = exe_dir.join("data");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn db_path() -> StorageResult<PathBuf> {
    Ok(data_dir()?.join("scratchpad.sqlite3"))
}

pub fn open_db() -> StorageResult<Connection> {
    let path = db_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&path)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    Ok(conn)
}
