use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use mime_guess::from_path as guess_mime;
use rusqlite::Connection;

use crate::content::models::ContentMutation;
use crate::models::entry::{DockEntry, EntryKind, EntryView};
use crate::scratchpad::storage::{
    create_dock_entry_internal, create_dock_entry_internal_with_revision,
};
use crate::storage::connection::data_dir;
use crate::storage::error::{StorageError, StorageResult};

const STAGED_ASSET_CREATE_ATTEMPTS: usize = 16;
type CleanupFn = fn(&Path) -> io::Result<()>;

fn sanitized_basename(original_name: &str) -> String {
    let mut safe_name = original_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_string();
    if safe_name == "." || safe_name == ".." || safe_name.is_empty() {
        safe_name = "untitled-file".to_string();
    }
    safe_name
}

fn random_asset_token() -> String {
    hex::encode(rand::random::<[u8; 16]>())
}

fn clear_readonly(path: &Path) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    let mut permissions = metadata.permissions();

    #[cfg(windows)]
    {
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() | 0o200);
    }

    #[cfg(not(any(windows, unix)))]
    permissions.set_readonly(false);

    fs::set_permissions(path, permissions)
}

fn cleanup_staged_asset(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            clear_readonly(path)?;
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(retry_error) if retry_error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(retry_error) => Err(retry_error),
            }
        }
        Err(error) => Err(error),
    }
}

struct StagedAsset {
    path: PathBuf,
    token: String,
    cleanup: CleanupFn,
    armed: bool,
}

impl StagedAsset {
    fn resolve<T>(mut self, result: StorageResult<T>) -> StorageResult<T> {
        match result {
            Ok(value) => {
                self.armed = false;
                Ok(value)
            }
            Err(import_error) => match (self.cleanup)(&self.path) {
                Ok(()) => {
                    self.armed = false;
                    Err(import_error)
                }
                Err(cleanup_error) => Err(StorageError::Other(format!(
                    "asset import failed: {import_error}; compensation failed for asset token {}: {cleanup_error}",
                    self.token
                ))),
            },
        }
    }
}

impl Drop for StagedAsset {
    fn drop(&mut self) {
        if self.armed {
            if let Err(error) = (self.cleanup)(&self.path) {
                eprintln!(
                    "failed to compensate staged asset token {}: {error}",
                    self.token
                );
            }
        }
    }
}

fn create_staged_file_with_token_source(
    dir: &Path,
    file_name: &str,
    cleanup: CleanupFn,
    mut next_token: impl FnMut() -> String,
) -> StorageResult<(StagedAsset, File)> {
    let safe_name = sanitized_basename(file_name);
    for _ in 0..STAGED_ASSET_CREATE_ATTEMPTS {
        let token = next_token();
        let path = dir.join(format!("{token}-{safe_name}"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                return Ok((
                    StagedAsset {
                        path,
                        token,
                        cleanup,
                        armed: true,
                    },
                    file,
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(StorageError::Io(error)),
        }
    }

    Err(StorageError::Other(format!(
        "could not allocate a staged asset after {STAGED_ASSET_CREATE_ATTEMPTS} exclusive attempts"
    )))
}

fn stage_bytes_with_token_source(
    dir: &Path,
    file_name: &str,
    bytes: &[u8],
    cleanup: CleanupFn,
    next_token: impl FnMut() -> String,
) -> StorageResult<StagedAsset> {
    let (staged, mut file) =
        create_staged_file_with_token_source(dir, file_name, cleanup, next_token)?;
    let write_result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = write_result {
        return staged.resolve(Err(StorageError::Io(error)));
    }
    Ok(staged)
}

fn stage_bytes(dir: &Path, file_name: &str, bytes: &[u8]) -> StorageResult<StagedAsset> {
    stage_bytes_with_token_source(dir, file_name, bytes, cleanup_staged_asset, || {
        random_asset_token()
    })
}

fn stage_file(source: &Path, dir: &Path, file_name: &str) -> StorageResult<(StagedAsset, i64)> {
    let mut source_file = File::open(source)?;
    let (staged, mut destination) = create_staged_file_with_token_source(
        dir,
        file_name,
        cleanup_staged_asset,
        random_asset_token,
    )?;
    let copy_result = io::copy(&mut source_file, &mut destination)
        .and_then(|size| destination.sync_all().map(|()| size));
    drop(destination);
    drop(source_file);
    let size = match copy_result {
        Ok(size) => size,
        Err(error) => return staged.resolve(Err(StorageError::Io(error))),
    };
    let size = i64::try_from(size)
        .map_err(|_| StorageError::Validation("imported asset is too large".to_string()));
    match size {
        Ok(size) => Ok((staged, size)),
        Err(error) => staged.resolve(Err(error)),
    }
}

struct PreparedAsset {
    staged: StagedAsset,
    kind: EntryKind,
    file_name: String,
    mime_type: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    size_bytes: i64,
    source: &'static str,
}

impl PreparedAsset {
    fn create_legacy(self, conn: &mut Connection, view: EntryView) -> StorageResult<DockEntry> {
        let file_path = self.staged.path.to_string_lossy().to_string();
        let result = create_dock_entry_internal(
            conn,
            view,
            self.kind,
            None,
            Some(file_path),
            Some(self.file_name),
            self.mime_type,
            self.width,
            self.height,
            Some(self.size_bytes),
            self.source,
        );
        self.staged.resolve(result)
    }

    fn create_with_revision(
        self,
        conn: &mut Connection,
        view: EntryView,
    ) -> StorageResult<ContentMutation<DockEntry>> {
        let file_path = self.staged.path.to_string_lossy().to_string();
        let result = create_dock_entry_internal_with_revision(
            conn,
            view,
            self.kind,
            None,
            Some(file_path),
            Some(self.file_name),
            self.mime_type,
            self.width,
            self.height,
            Some(self.size_bytes),
            self.source,
        );
        self.staged.resolve(result)
    }
}

fn prepare_file(source_path: &str) -> StorageResult<PreparedAsset> {
    let source = Path::new(source_path);
    let file_name = source
        .file_name()
        .map(|name| sanitized_basename(&name.to_string_lossy()))
        .unwrap_or_else(|| "imported-file".to_string());
    let mime_type = guess_mime(source).first_raw().map(str::to_string);
    let kind = classify_kind(source, mime_type.as_deref());
    let (staged, size_bytes) = stage_file(source, &assets_dir()?, &file_name)?;
    Ok(PreparedAsset {
        staged,
        kind,
        file_name,
        mime_type,
        width: None,
        height: None,
        size_bytes,
        source: "drop",
    })
}

fn prepare_image_bytes(
    bytes: &[u8],
    file_name: &str,
    mime_type: &str,
    width: Option<i64>,
    height: Option<i64>,
) -> StorageResult<PreparedAsset> {
    let safe_name = sanitized_basename(file_name);
    let staged = stage_bytes(&assets_dir()?, &safe_name, bytes)?;
    Ok(PreparedAsset {
        staged,
        kind: EntryKind::Image,
        file_name: safe_name,
        mime_type: Some(mime_type.to_string()),
        width,
        height,
        size_bytes: bytes.len() as i64,
        source: "clipboard",
    })
}

fn prepare_file_bytes(
    bytes: &[u8],
    file_name: &str,
    mime_type: Option<&str>,
) -> StorageResult<PreparedAsset> {
    let safe_name = sanitized_basename(file_name);
    let kind = classify_kind(Path::new(&safe_name), mime_type);
    let staged = stage_bytes(&assets_dir()?, &safe_name, bytes)?;
    Ok(PreparedAsset {
        staged,
        kind,
        file_name: safe_name,
        mime_type: mime_type.map(str::to_string),
        width: None,
        height: None,
        size_bytes: bytes.len() as i64,
        source: "drop",
    })
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
    prepare_file(source_path)?.create_legacy(conn, view)
}

pub fn import_file_with_revision(
    conn: &mut Connection,
    source_path: &str,
    view: EntryView,
) -> StorageResult<ContentMutation<DockEntry>> {
    prepare_file(source_path)?.create_with_revision(conn, view)
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
    prepare_image_bytes(bytes, file_name, mime_type, width, height)?.create_legacy(conn, view)
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
    prepare_image_bytes(bytes, file_name, mime_type, width, height)?
        .create_with_revision(conn, view)
}

pub fn import_file_bytes(
    conn: &mut Connection,
    bytes: &[u8],
    file_name: &str,
    mime_type: Option<&str>,
    view: EntryView,
) -> StorageResult<DockEntry> {
    prepare_file_bytes(bytes, file_name, mime_type)?.create_legacy(conn, view)
}

pub fn import_file_bytes_with_revision(
    conn: &mut Connection,
    bytes: &[u8],
    file_name: &str,
    mime_type: Option<&str>,
    view: EntryView,
) -> StorageResult<ContentMutation<DockEntry>> {
    prepare_file_bytes(bytes, file_name, mime_type)?.create_with_revision(conn, view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dock_only_connection() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::scratchpad::storage::ensure_dock_schema(&mut conn).unwrap();
        conn
    }

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

    fn isolated_asset_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "scratchpad-asset-{label}-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn injected_cleanup_failure(_path: &Path) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "injected cleanup failure",
        ))
    }

    #[test]
    fn exclusive_staging_retries_collision_without_overwriting_or_deleting_existing_file() {
        let dir = isolated_asset_dir("collision");
        let collision = dir.join("fixed-report.bin");
        fs::write(&collision, b"preexisting sentinel").unwrap();
        let mut tokens = ["fixed", "fresh"].into_iter();

        let staged = stage_bytes_with_token_source(
            &dir,
            "report.bin",
            b"new import",
            cleanup_staged_asset,
            || tokens.next().unwrap().to_string(),
        )
        .unwrap();
        let staged_path = staged.path.clone();
        let error = staged
            .resolve::<()>(Err(crate::storage::error::StorageError::Other(
                "forced database failure".to_string(),
            )))
            .unwrap_err();

        assert!(error.to_string().contains("forced database failure"));
        assert_eq!(fs::read(&collision).unwrap(), b"preexisting sentinel");
        assert!(!staged_path.exists());
        fs::remove_file(collision).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn cleanup_failure_is_returned_without_exposing_the_asset_directory() {
        let dir = isolated_asset_dir("cleanup-failure");
        let mut tokens = ["safe-token"].into_iter();
        let staged = stage_bytes_with_token_source(
            &dir,
            "secret.bin",
            b"staged",
            injected_cleanup_failure,
            || tokens.next().unwrap().to_string(),
        )
        .unwrap();
        let staged_path = staged.path.clone();

        let error = staged
            .resolve::<()>(Err(crate::storage::error::StorageError::Other(
                "forced database failure".to_string(),
            )))
            .unwrap_err();
        let message = error.to_string();

        assert!(message.contains("forced database failure"));
        assert!(message.contains("compensation failed"));
        assert!(message.contains("safe-token"));
        assert!(!message.contains(dir.to_string_lossy().as_ref()));
        fs::remove_file(staged_path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn readonly_staged_asset_is_removed_when_database_creation_fails() {
        let dir = isolated_asset_dir("readonly-cleanup");
        let mut tokens = ["readonly-token"].into_iter();
        let staged = stage_bytes_with_token_source(
            &dir,
            "readonly.bin",
            b"staged",
            cleanup_staged_asset,
            || tokens.next().unwrap().to_string(),
        )
        .unwrap();
        let staged_path = staged.path.clone();
        let mut permissions = fs::metadata(&staged_path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&staged_path, permissions).unwrap();

        let error = staged
            .resolve::<()>(Err(crate::storage::error::StorageError::Other(
                "forced database failure".to_string(),
            )))
            .unwrap_err();

        assert!(error.to_string().contains("forced database failure"));
        assert!(!error.to_string().contains("compensation failed"));
        assert!(!staged_path.exists());
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn dock_only_legacy_import_apis_keep_their_dock_entry_contract() {
        let mut conn = dock_only_connection();
        let source_dir = std::env::temp_dir().join(format!(
            "scratchpad-dock-only-import-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("legacy-source.bin");
        fs::write(&source, b"legacy source").unwrap();

        let copied = import_file(
            &mut conn,
            source.to_string_lossy().as_ref(),
            EntryView::Home,
        )
        .unwrap();
        let image = import_image_bytes(
            &mut conn,
            b"legacy image",
            "legacy-image.png",
            "image/png",
            None,
            None,
            EntryView::Home,
        )
        .unwrap();
        let file = import_file_bytes(
            &mut conn,
            b"legacy file",
            "legacy-file.pdf",
            Some("application/pdf"),
            EntryView::Home,
        )
        .unwrap();

        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM entries", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='content_catalog')",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
        for entry in [&copied, &image, &file] {
            assert!(Path::new(entry.file_path.as_deref().unwrap()).is_file());
            crate::scratchpad::storage::remove_from_view(
                &mut conn,
                EntryView::Home,
                &entry.id,
            )
            .unwrap();
        }
        assert!(import_file_with_revision(
            &mut conn,
            source.to_string_lossy().as_ref(),
            EntryView::Home,
        )
        .is_err());
        assert!(import_image_bytes_with_revision(
            &mut conn,
            b"unified image",
            "unified-image.png",
            "image/png",
            None,
            None,
            EntryView::Home,
        )
        .is_err());
        assert!(import_file_bytes_with_revision(
            &mut conn,
            b"unified file",
            "unified-file.pdf",
            Some("application/pdf"),
            EntryView::Home,
        )
        .is_err());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM entries", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(source.is_file());
        fs::remove_file(source).unwrap();
        fs::remove_dir(source_dir).unwrap();
    }

    #[test]
    fn byte_import_persists_only_a_sanitized_basename() {
        let mut conn = unified_connection();
        let imported = import_file_bytes_with_revision(
            &mut conn,
            b"safe filename",
            r"C:\Users\Alice\Secret\photo.png",
            Some("image/png"),
            EntryView::Home,
        )
        .unwrap();
        let unified_id = format!("dock:{}", imported.value.id);

        assert_eq!(imported.value.file_name.as_deref(), Some("photo.png"));
        let projection: (String, String) = conn
            .query_row(
                "SELECT title, body FROM content_fts WHERE unified_id=?1",
                rusqlite::params![unified_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(!format!("{} {}", projection.0, projection.1).contains("Alice"));
        assert!(!format!("{} {}", projection.0, projection.1).contains("Secret"));

        crate::scratchpad::storage::remove_from_view(
            &mut conn,
            EntryView::Home,
            &imported.value.id,
        )
        .unwrap();
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
