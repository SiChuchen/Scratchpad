use super::error::{StorageError, StorageResult};
use rusqlite::Connection;
use std::path::PathBuf;
use std::{fs, io};

/// Returns the directory for non-data config files (override, etc).
/// Portable: `<exe_dir>/`     Installed: `%LOCALAPPDATA%/Soma Scratchpad/`
fn config_home() -> StorageResult<PathBuf> {
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
    let portable_data = exe_dir.join("data");
    if portable_data.exists() {
        return Ok(exe_dir.to_path_buf());
    }
    let appdata = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
        // Fallback: use exe_dir if LOCALAPPDATA is not set (rare)
        exe_dir.to_string_lossy().to_string()
    });
    let dir = PathBuf::from(appdata).join("Soma Scratchpad");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn override_path() -> StorageResult<PathBuf> {
    Ok(config_home()?.join("datadir_override.cfg"))
}

fn read_override() -> Option<PathBuf> {
    let path = override_path().ok()?;
    let content = fs::read_to_string(&path).ok()?;
    let dir = PathBuf::from(content.trim());
    if dir.as_os_str().is_empty() {
        return None;
    }
    // Verify the directory is usable
    if dir.exists() || fs::create_dir_all(&dir).is_ok() {
        Some(dir)
    } else {
        None
    }
}

pub fn save_data_dir_override(dir: &str) -> StorageResult<()> {
    let path = override_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, dir)?;
    Ok(())
}

/// Returns the data directory for the application.
///
/// Resolution order:
/// 1. `datadir_override.cfg` in config home (user-chosen custom path)
/// 2. `<exe_dir>/data/` if it already exists (portable mode)
/// 3. `%LOCALAPPDATA%/Soma Scratchpad/data/` (installed mode)
pub fn data_dir() -> StorageResult<PathBuf> {
    // 1. Custom override
    if let Some(override_dir) = read_override() {
        return Ok(override_dir);
    }

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
    let portable = exe_dir.join("data");

    // 2. Portable mode: exe-adjacent data/ already exists
    if portable.exists() {
        fs::create_dir_all(&portable)?;
        return Ok(portable);
    }

    // 3. Installed mode: AppData/Local
    let appdata =
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| exe_dir.to_string_lossy().to_string());
    let dir = PathBuf::from(appdata).join("Soma Scratchpad").join("data");
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
