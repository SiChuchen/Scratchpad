use std::{fs, io, path::PathBuf};
use rusqlite::Connection;
use super::error::{StorageError, StorageResult};

pub fn db_path() -> StorageResult<PathBuf> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        StorageError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "failed to resolve LocalAppData directory",
        ))
    })?;
    Ok(base.join("Soma").join("scratchpad").join("scratchpad.sqlite3"))
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
