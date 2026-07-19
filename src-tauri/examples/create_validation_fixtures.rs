use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rand::{rngs::OsRng, RngCore};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use soma_scratchpad::content::migrations::ensure_content_schema;
use soma_scratchpad::scratchpad::storage::ensure_dock_schema;
use soma_scratchpad::vault::storage::ensure_vault_schema;

const LEGACY_NAME: &str = "legacy-validation.sqlite3";
const FRESH_NAME: &str = "fresh-validation.sqlite3";
const STAGING_PREFIX: &str = ".unified-validation-staging-";
const OWNER_FILE: &str = ".fixture-owner";

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os();
    let program = arguments
        .next()
        .and_then(|value| PathBuf::from(value).file_name().map(|name| name.to_owned()))
        .unwrap_or_else(|| "create_validation_fixtures".into());
    let Some(output_directory) = arguments.next() else {
        return Err(format!("usage: {} <output-directory>", program.to_string_lossy()).into());
    };
    if arguments.next().is_some() {
        return Err(format!("usage: {} <output-directory>", program.to_string_lossy()).into());
    }

    let output_directory = PathBuf::from(output_directory);
    let (legacy, fresh) = generate_fixtures(&output_directory, |_, _| Ok(()))?;
    let targets = fixture_paths(&output_directory);

    println!(
        "legacy: {} (main v{}, vault v{}, {} payloads, content schema absent)",
        targets[0].display(),
        legacy.0,
        legacy.1,
        legacy.2
    );
    println!(
        "fresh: {} (main v{}, vault v{}, {} catalog, {} temporary, {} saved, \
         {} content_fts, {} vault_fts, revision {}, {} pending delete)",
        targets[1].display(),
        fresh.0,
        fresh.1,
        fresh.2,
        fresh.3,
        fresh.4,
        fresh.5,
        fresh.6,
        fresh.7,
        fresh.8
    );
    Ok(())
}

fn fixture_paths(directory: &Path) -> [PathBuf; 2] {
    [directory.join(LEGACY_NAME), directory.join(FRESH_NAME)]
}

fn generate_fixtures<F>(
    output_directory: &Path,
    before_publish: F,
) -> Result<(LegacyVerification, FreshVerification), Box<dyn std::error::Error>>
where
    F: FnOnce(&Path, &[PathBuf; 2]) -> Result<(), Box<dyn std::error::Error>>,
{
    generate_fixtures_with_failures(
        output_directory,
        OwnerFailure::None,
        PublishFailure::None,
        before_publish,
    )
}

fn generate_fixtures_with_failures<F>(
    output_directory: &Path,
    owner_failure: OwnerFailure,
    publish_failure: PublishFailure,
    before_publish: F,
) -> Result<(LegacyVerification, FreshVerification), Box<dyn std::error::Error>>
where
    F: FnOnce(&Path, &[PathBuf; 2]) -> Result<(), Box<dyn std::error::Error>>,
{
    reject_existing_output(output_directory)?;
    let parent = output_directory.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    reject_existing_output(output_directory)?;

    let mut staging = StagingGuard::create_with_failure(parent, owner_failure)?;
    let targets = fixture_paths(staging.path());
    create_legacy_fixture(&targets[0])?;
    create_fresh_fixture(&targets[1])?;
    let legacy = verify_legacy_fixture(&targets[0])?;
    let fresh = verify_fresh_fixture(&targets[1])?;
    before_publish(staging.path(), &targets)?;
    staging.prepare_publish(publish_failure)?;
    fs::rename(staging.path(), output_directory)?;
    staging.disarm();
    Ok((legacy, fresh))
}

fn reject_existing_output(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(format!("refusing to replace existing output: {}", path.display()).into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnerFailure {
    None,
    BeforeOpen,
    DuringWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublishFailure {
    None,
    BeforeOwnerRemove,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StagingState {
    DirOnly,
    MarkerComplete,
    PublishReady,
}

struct StagingGuard {
    parent: PathBuf,
    path: PathBuf,
    owner: [u8; 32],
    owner_file_created: bool,
    owner_bytes_written: usize,
    state: StagingState,
    armed: bool,
}

impl StagingGuard {
    fn create_with_failure(
        parent: &Path,
        failure: OwnerFailure,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        for _ in 0..128 {
            let mut random = [0_u8; 32];
            OsRng.fill_bytes(&mut random);
            let name = format!("{STAGING_PREFIX}{}", hex::encode(random));
            let path = parent.join(name);
            let mut owner = [0_u8; 32];
            OsRng.fill_bytes(&mut owner);
            match fs::create_dir(&path) {
                Ok(()) => {
                    let mut guard = Self {
                        parent: parent.to_path_buf(),
                        path,
                        owner,
                        owner_file_created: false,
                        owner_bytes_written: 0,
                        state: StagingState::DirOnly,
                        armed: true,
                    };
                    guard.initialize_owner(failure)?;
                    return Ok(guard);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err("could not reserve a unique staging directory".into())
    }

    fn initialize_owner(
        &mut self,
        failure: OwnerFailure,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if failure == OwnerFailure::BeforeOpen {
            return Err(io::Error::other("injected owner open failure").into());
        }
        let owner_path = self.path.join(OWNER_FILE);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options.open(owner_path)?;
        self.owner_file_created = true;

        use std::io::Write;
        while self.owner_bytes_written < self.owner.len() {
            let end = if failure == OwnerFailure::DuringWrite {
                (self.owner_bytes_written + 8).min(self.owner.len())
            } else {
                self.owner.len()
            };
            let written = file.write(&self.owner[self.owner_bytes_written..end])?;
            if written == 0 {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "owner marker").into());
            }
            self.owner_bytes_written += written;
            if failure == OwnerFailure::DuringWrite {
                return Err(io::Error::other("injected owner write failure").into());
            }
        }
        file.sync_all()?;
        self.state = StagingState::MarkerComplete;
        Ok(())
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    fn valid_staging_path(&self) -> bool {
        self.path.parent() == Some(self.parent.as_path())
            && self
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(STAGING_PREFIX) && name.len() == STAGING_PREFIX.len() + 64
                })
            && fs::symlink_metadata(&self.path)
                .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
                .unwrap_or(false)
    }

    fn owns_staging(&self) -> bool {
        self.state == StagingState::MarkerComplete
            && self.valid_staging_path()
            && fs::read(self.path.join(OWNER_FILE))
                .map(|bytes| bytes == self.owner)
                .unwrap_or(false)
    }

    fn prepare_publish(
        &mut self,
        failure: PublishFailure,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.owns_staging() {
            return Err("staging ownership check failed before publish".into());
        }
        let mut names = fs::read_dir(&self.path)?
            .map(|entry| entry.map(|value| value.file_name()))
            .collect::<io::Result<Vec<_>>>()?;
        names.sort();
        let mut expected: [std::ffi::OsString; 3] =
            [FRESH_NAME.into(), LEGACY_NAME.into(), OWNER_FILE.into()];
        expected.sort();
        if names != expected {
            return Err("staging contains unexpected files before publish".into());
        }
        if failure == PublishFailure::BeforeOwnerRemove {
            return Err(io::Error::other("injected owner remove failure").into());
        }
        fs::remove_file(self.path.join(OWNER_FILE))?;
        self.state = StagingState::PublishReady;
        Ok(())
    }

    fn cleanup(&self) {
        match self.state {
            StagingState::DirOnly => {
                if self.valid_staging_path() && self.owner_file_created {
                    let owner_path = self.path.join(OWNER_FILE);
                    let expected = &self.owner[..self.owner_bytes_written];
                    if fs::read(&owner_path)
                        .map(|bytes| bytes == expected)
                        .unwrap_or(false)
                        && fs::symlink_metadata(&owner_path)
                            .map(|metadata| {
                                metadata.is_file() && !metadata.file_type().is_symlink()
                            })
                            .unwrap_or(false)
                    {
                        let _ = fs::remove_file(owner_path);
                    }
                }
                if self.valid_staging_path() {
                    let _ = fs::remove_dir(&self.path);
                }
            }
            StagingState::MarkerComplete if self.owns_staging() => {
                let _ = fs::remove_dir_all(&self.path);
            }
            StagingState::PublishReady if self.valid_staging_path() => {
                let _ = fs::remove_dir_all(&self.path);
            }
            _ => {}
        }
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if self.armed {
            self.cleanup();
        }
    }
}

type LegacyVerification = (i64, i64, i64);

struct LogicalTable {
    name: &'static str,
    sql: &'static str,
}

const COMMON_LOGICAL_TABLES: &[LogicalTable] = &[
    LogicalTable {
        name: "entries",
        sql: "SELECT id, kind, content, file_path, file_name, mime_type, width, height,
                     size_bytes, collapsed, source, created_at, updated_at, title
              FROM entries ORDER BY id",
    },
    LogicalTable {
        name: "home_entries",
        sql: "SELECT entry_id, created_at, sort_order FROM home_entries ORDER BY entry_id",
    },
    LogicalTable {
        name: "note_entries",
        sql: "SELECT entry_id, created_at, sort_order FROM note_entries ORDER BY entry_id",
    },
    LogicalTable {
        name: "vault_entries",
        sql: "SELECT id, kind, title, notes, created_at, updated_at
              FROM vault_entries ORDER BY id",
    },
    LogicalTable {
        name: "vault_fields",
        sql: "SELECT id, entry_id, key, value, is_sensitive, sort_order
              FROM vault_fields ORDER BY entry_id, sort_order, id",
    },
    LogicalTable {
        name: "vault_tags",
        sql: "SELECT entry_id, tag, normalized_tag, source
              FROM vault_tags ORDER BY entry_id, normalized_tag, source",
    },
    LogicalTable {
        name: "vault_ai_metadata",
        sql: "SELECT entry_id, summary, search_aliases_json, content_hash,
                     provider_id, model, generated_at, status
              FROM vault_ai_metadata ORDER BY entry_id",
    },
    LogicalTable {
        name: "vault_capture_requests",
        sql: "SELECT request_id, entry_id, created_at
              FROM vault_capture_requests ORDER BY request_id",
    },
    LogicalTable {
        name: "vault_fts",
        sql: "SELECT entry_id, title, notes, searchable
              FROM vault_fts ORDER BY entry_id, rowid",
    },
];

const FRESH_ONLY_LOGICAL_TABLES: &[LogicalTable] = &[
    LogicalTable {
        name: "content_catalog",
        sql: "SELECT unified_id, source, source_id, kind, retention_state,
                     retention_changed_at, cleanup_at, inbox_position, saved_position,
                     created_at, updated_at
              FROM content_catalog ORDER BY unified_id",
    },
    LogicalTable {
        name: "content_fts",
        sql: "SELECT unified_id, title, body, tags, aliases
              FROM content_fts ORDER BY unified_id, rowid",
    },
    LogicalTable {
        name: "content_state",
        sql: "SELECT singleton, revision FROM content_state ORDER BY singleton",
    },
    LogicalTable {
        name: "content_pending_deletes",
        sql: "SELECT token, unified_id, created_at, expires_at, status
              FROM content_pending_deletes ORDER BY token",
    },
];

const LEGACY_LOGICAL_DIGESTS: &[(&str, &str)] = &[
    (
        "entries",
        "97983bbfc8fdc9bfa652cb2710e61054fbd00b8f18f802901f44471307d1e591",
    ),
    (
        "home_entries",
        "a94b2eb2a2534a44872e6ac9ab2eaa2c34f7dbf6144ef297fc0150980fb65d20",
    ),
    (
        "note_entries",
        "0293d9e3de3be83efcf085936df10f6dbbc316f23380777a4b47e55997a6c8f4",
    ),
    (
        "vault_entries",
        "7c1106e9740103204b6e9033c90271ac6c6419460352f03da0af996e2a2f29f0",
    ),
    (
        "vault_fields",
        "0df8e88dc6a138f2201e2e7312a6c1fe897d8136baba13a9e36efb9324e0414d",
    ),
    (
        "vault_tags",
        "b6158aed917ce5f8b1dd3b1d4f4312fee3b2cf9d11e51d5bdad7beda5ba546dc",
    ),
    (
        "vault_ai_metadata",
        "452f357448a722a1549394efe1038ac5342bf6f7592bb8fb8181822acb1688e1",
    ),
    (
        "vault_capture_requests",
        "8e3022ac77b0383733417508e4cd495e6215704bdb428768835e829ac079ebe6",
    ),
    (
        "vault_fts",
        "bff5ec59792d17bf6056016a89c98ff1a2db5276429c6eef3c61334ef8154369",
    ),
];

const FRESH_LOGICAL_DIGESTS: &[(&str, &str)] = &[
    (
        "entries",
        "97983bbfc8fdc9bfa652cb2710e61054fbd00b8f18f802901f44471307d1e591",
    ),
    (
        "home_entries",
        "a94b2eb2a2534a44872e6ac9ab2eaa2c34f7dbf6144ef297fc0150980fb65d20",
    ),
    (
        "note_entries",
        "0293d9e3de3be83efcf085936df10f6dbbc316f23380777a4b47e55997a6c8f4",
    ),
    (
        "vault_entries",
        "7c1106e9740103204b6e9033c90271ac6c6419460352f03da0af996e2a2f29f0",
    ),
    (
        "vault_fields",
        "0df8e88dc6a138f2201e2e7312a6c1fe897d8136baba13a9e36efb9324e0414d",
    ),
    (
        "vault_tags",
        "b6158aed917ce5f8b1dd3b1d4f4312fee3b2cf9d11e51d5bdad7beda5ba546dc",
    ),
    (
        "vault_ai_metadata",
        "452f357448a722a1549394efe1038ac5342bf6f7592bb8fb8181822acb1688e1",
    ),
    (
        "vault_capture_requests",
        "8e3022ac77b0383733417508e4cd495e6215704bdb428768835e829ac079ebe6",
    ),
    (
        "vault_fts",
        "bff5ec59792d17bf6056016a89c98ff1a2db5276429c6eef3c61334ef8154369",
    ),
    (
        "content_catalog",
        "7bdb5486a609a3ccc175a888c68d69d3cc7ef884abdfa13c1aac3f1ecd9af090",
    ),
    (
        "content_fts",
        "2ced1a25a8139f8455796991e684dd4c8b0ab946a0d16b382d641944a020f816",
    ),
    (
        "content_state",
        "00b63d4dc309d676b681e3a6504c7ca4f8e07d74ffdc513b66e7e79b2287400a",
    ),
    (
        "content_pending_deletes",
        "2815e71f7fdcabbd206e1ee19da289f178aecf4d360fdb4ccc1c56721069bee6",
    ),
];

fn read_only(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
}

fn logical_table_digest(connection: &Connection, table: &LogicalTable) -> rusqlite::Result<String> {
    let mut statement = connection.prepare(table.sql)?;
    let column_count = statement.column_count();
    let mut rows = statement.query([])?;
    let mut digest = Sha256::new();
    digest.update(b"scratchpad-logical-table-v1\0");
    while let Some(row) = rows.next()? {
        digest.update(b"R");
        digest.update((column_count as u64).to_be_bytes());
        for column in 0..column_count {
            match row.get_ref(column)? {
                ValueRef::Null => digest.update(b"N"),
                ValueRef::Integer(value) => {
                    digest.update(b"I");
                    digest.update(value.to_be_bytes());
                }
                ValueRef::Real(value) => {
                    digest.update(b"R");
                    digest.update(value.to_bits().to_be_bytes());
                }
                ValueRef::Text(value) => {
                    digest.update(b"T");
                    digest.update((value.len() as u64).to_be_bytes());
                    digest.update(value);
                }
                ValueRef::Blob(value) => {
                    digest.update(b"B");
                    digest.update((value.len() as u64).to_be_bytes());
                    digest.update(value);
                }
            }
        }
    }
    Ok(hex::encode(digest.finalize()))
}

fn verify_logical_tables(
    connection: &Connection,
    tables: &[LogicalTable],
    expected: &[(&str, &str)],
) -> Result<(), Box<dyn std::error::Error>> {
    if tables.len() != expected.len() {
        return Err("logical digest table set is incomplete".into());
    }
    for (table, (expected_name, expected_digest)) in tables.iter().zip(expected) {
        if table.name != *expected_name {
            return Err(format!("logical digest table order mismatch: {}", table.name).into());
        }
        let actual = logical_table_digest(connection, table)?;
        if actual != *expected_digest {
            return Err(format!(
                "logical digest mismatch for {}: expected {}, got {}",
                table.name, expected_digest, actual
            )
            .into());
        }
    }
    Ok(())
}

fn verify_legacy_fixture(path: &Path) -> Result<LegacyVerification, Box<dyn std::error::Error>> {
    let connection = read_only(path)?;
    verify_logical_tables(&connection, COMMON_LOGICAL_TABLES, LEGACY_LOGICAL_DIGESTS)?;
    let main_version = connection.query_row(
        "SELECT version FROM schema_version WHERE scope='main'",
        [],
        |row| row.get(0),
    )?;
    let vault_version =
        connection.query_row("SELECT version FROM vault_schema_version", [], |row| {
            row.get(0)
        })?;
    let payloads = connection.query_row(
        "SELECT (SELECT COUNT(*) FROM entries) +
                (SELECT COUNT(*) FROM vault_entries)",
        [],
        |row| row.get(0),
    )?;
    let dock = strings(
        &connection,
        "SELECT id || ':' || kind FROM entries ORDER BY id",
    )?;
    let home = memberships(&connection, "home_entries")?;
    let note = memberships(&connection, "note_entries")?;
    let vault = strings(
        &connection,
        "SELECT id || ':' || kind FROM vault_entries ORDER BY id",
    )?;
    let fields = strings(
        &connection,
        "SELECT id || '|' || entry_id || '|' || key || '|' || value || '|' ||
                is_sensitive || '|' || sort_order
         FROM vault_fields ORDER BY entry_id, sort_order, id",
    )?;
    let tags = strings(
        &connection,
        "SELECT entry_id || '|' || tag || '|' || normalized_tag || '|' || source
         FROM vault_tags ORDER BY entry_id, normalized_tag, source",
    )?;
    let metadata = strings(
        &connection,
        "SELECT entry_id || '|' || summary || '|' || search_aliases_json || '|' ||
                content_hash || '|' || status
         FROM vault_ai_metadata ORDER BY entry_id",
    )?;
    let vault_fts_ids = id_counts(&connection, "vault_fts", "entry_id")?;
    let content_schema = count(
        &connection,
        "SELECT COUNT(*) FROM sqlite_master
         WHERE name IN ('content_catalog', 'content_state',
                        'content_pending_deletes', 'content_fts')",
    )?;
    let unsafe_fts = count(
        &connection,
        "SELECT COUNT(*) FROM vault_fts
         WHERE lower(title || notes || searchable) LIKE '%neverindexme%'",
    )?;
    let useful_fts = count(
        &connection,
        "SELECT COUNT(*) FROM vault_fts
         WHERE entry_id='credential-legacy' AND searchable LIKE '%fixture-user%'
           AND searchable LIKE '%Manual Tag%'",
    )?;
    let foreign_key_errors = count(&connection, "SELECT COUNT(*) FROM pragma_foreign_key_check")?;
    if (main_version, vault_version, payloads, content_schema) != (2, 4, 7, 0)
        || dock
            != [
                "dual-image:image",
                "home-text:text",
                "note-file:file",
            ]
        || home != [("dual-image".into(), 1.5), ("home-text".into(), 3.0)]
        || note != [("dual-image".into(), 2.5), ("note-file".into(), 0.5)]
        || vault
            != [
                "bookmark-legacy:bookmark",
                "credential-legacy:credential",
                "credential-secondary:credential",
                "note-legacy:note",
            ]
        || fields
            != [
                "field-url|bookmark-legacy|url|https://fixture.invalid|0|0",
                "field-user|credential-legacy|username|fixture-user|0|0",
                "field-password|credential-legacy| password |NeverIndexMePassword|1|1",
                "field-token|credential-secondary| ToKeN |NeverIndexMeToken|1|0",
            ]
        || tags
            != [
                "bookmark-legacy|AI Tag|ai tag|ai",
                "credential-legacy|Manual Tag|manual tag|manual",
            ]
        || metadata
            != [
                "bookmark-legacy|approved safe summary|[\"validation portal\"]|bookmark-validation-hash|ready",
                "credential-legacy|pending private prompt output|[\"pending alias\"]|credential-validation-hash|pending",
            ]
        || vault_fts_ids
            != [
                ("bookmark-legacy".into(), 1),
                ("credential-legacy".into(), 1),
                ("credential-secondary".into(), 1),
                ("note-legacy".into(), 1),
            ]
        || unsafe_fts != 0
        || useful_fts != 1
        || foreign_key_errors != 0
    {
        return Err("legacy fixture verification failed".into());
    }
    Ok((main_version, vault_version, payloads))
}

type FreshVerification = (i64, i64, i64, i64, i64, i64, i64, i64, i64);

fn verify_fresh_fixture(path: &Path) -> Result<FreshVerification, Box<dyn std::error::Error>> {
    let connection = read_only(path)?;
    verify_logical_tables(
        &connection,
        COMMON_LOGICAL_TABLES,
        &FRESH_LOGICAL_DIGESTS[..COMMON_LOGICAL_TABLES.len()],
    )?;
    verify_logical_tables(
        &connection,
        FRESH_ONLY_LOGICAL_TABLES,
        &FRESH_LOGICAL_DIGESTS[COMMON_LOGICAL_TABLES.len()..],
    )?;
    verify_fixed_payloads(&connection)?;
    let main_version = connection.query_row(
        "SELECT version FROM schema_version WHERE scope='main'",
        [],
        |row| row.get(0),
    )?;
    let vault_version =
        connection.query_row("SELECT version FROM vault_schema_version", [], |row| {
            row.get(0)
        })?;
    let catalog = count(&connection, "SELECT COUNT(*) FROM content_catalog")?;
    let temporary = count(
        &connection,
        "SELECT COUNT(*) FROM content_catalog WHERE retention_state='temporary'",
    )?;
    let saved = count(
        &connection,
        "SELECT COUNT(*) FROM content_catalog WHERE retention_state='saved'",
    )?;
    let content_fts = count(&connection, "SELECT COUNT(*) FROM content_fts")?;
    let vault_fts = count(&connection, "SELECT COUNT(*) FROM vault_fts")?;
    let revision = count(
        &connection,
        "SELECT revision FROM content_state WHERE singleton=1",
    )?;
    let pending_columns = strings(
        &connection,
        "SELECT name FROM pragma_table_info('content_pending_deletes') ORDER BY cid",
    )?;
    let pending_row: (String, String, String, String, String) = connection.query_row(
        "SELECT token, unified_id, created_at, expires_at, status
         FROM content_pending_deletes",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    let pending = count(&connection, "SELECT COUNT(*) FROM content_pending_deletes")?;
    let unsafe_fts = count(
        &connection,
        "SELECT
            (SELECT COUNT(*) FROM content_fts
             WHERE lower(title || body || tags || aliases) LIKE '%neverindexme%') +
            (SELECT COUNT(*) FROM vault_fts
             WHERE lower(title || notes || searchable) LIKE '%neverindexme%')",
    )?;
    let catalog_rows = catalog_identity(&connection)?;
    let content_fts_ids = id_counts(&connection, "content_fts", "unified_id")?;
    let vault_fts_ids = id_counts(&connection, "vault_fts", "entry_id")?;
    let useful_fts = count(
        &connection,
        "SELECT
            (SELECT COUNT(*) FROM vault_fts
             WHERE entry_id='credential-legacy' AND searchable LIKE '%fixture-user%') +
            (SELECT COUNT(*) FROM content_fts
             WHERE unified_id='dock:home-text' AND body LIKE '%Validation home text%') +
            (SELECT COUNT(*) FROM content_fts
             WHERE unified_id='vault:bookmark-legacy'
               AND aliases LIKE '%validation portal%')",
    )?;
    let foreign_key_errors = count(&connection, "SELECT COUNT(*) FROM pragma_foreign_key_check")?;
    if (
        main_version,
        vault_version,
        catalog,
        temporary,
        saved,
        content_fts,
        vault_fts,
        revision,
        pending,
        unsafe_fts,
    ) != (4, 4, 7, 2, 5, 7, 4, 9, 1, 0)
        || pending_columns != ["token", "unified_id", "created_at", "expires_at", "status"]
        || pending_row
            != (
                "validation-delete-token-000000000001".into(),
                "dock:home-text".into(),
                "2026-07-18T12:00:00+00:00".into(),
                "2026-07-18T12:00:10+00:00".into(),
                "pending".into(),
            )
        || catalog_rows != expected_catalog_identity()
        || content_fts_ids
            != [
                ("dock:dual-image".into(), 1),
                ("dock:home-text".into(), 1),
                ("dock:note-file".into(), 1),
                ("vault:bookmark-legacy".into(), 1),
                ("vault:credential-legacy".into(), 1),
                ("vault:credential-secondary".into(), 1),
                ("vault:note-legacy".into(), 1),
            ]
        || vault_fts_ids
            != [
                ("bookmark-legacy".into(), 1),
                ("credential-legacy".into(), 1),
                ("credential-secondary".into(), 1),
                ("note-legacy".into(), 1),
            ]
        || useful_fts != 3
        || foreign_key_errors != 0
    {
        return Err("fresh fixture verification failed".into());
    }
    Ok((
        main_version,
        vault_version,
        catalog,
        temporary,
        saved,
        content_fts,
        vault_fts,
        revision,
        pending,
    ))
}

fn verify_fixed_payloads(connection: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let dock = strings(
        connection,
        "SELECT id || ':' || kind FROM entries ORDER BY id",
    )?;
    let home = memberships(connection, "home_entries")?;
    let note = memberships(connection, "note_entries")?;
    let vault = strings(
        connection,
        "SELECT id || ':' || kind FROM vault_entries ORDER BY id",
    )?;
    let fields = strings(
        connection,
        "SELECT id || '|' || entry_id || '|' || key || '|' || value || '|' ||
                is_sensitive || '|' || sort_order
         FROM vault_fields ORDER BY entry_id, sort_order, id",
    )?;
    let tags = strings(
        connection,
        "SELECT entry_id || '|' || tag || '|' || normalized_tag || '|' || source
         FROM vault_tags ORDER BY entry_id, normalized_tag, source",
    )?;
    let metadata = strings(
        connection,
        "SELECT entry_id || '|' || summary || '|' || search_aliases_json || '|' ||
                content_hash || '|' || status
         FROM vault_ai_metadata ORDER BY entry_id",
    )?;

    if dock != ["dual-image:image", "home-text:text", "note-file:file"]
        || home != [("dual-image".into(), 1.5), ("home-text".into(), 3.0)]
        || note != [("dual-image".into(), 2.5), ("note-file".into(), 0.5)]
        || vault
            != [
                "bookmark-legacy:bookmark",
                "credential-legacy:credential",
                "credential-secondary:credential",
                "note-legacy:note",
            ]
        || fields
            != [
                "field-url|bookmark-legacy|url|https://fixture.invalid|0|0",
                "field-user|credential-legacy|username|fixture-user|0|0",
                "field-password|credential-legacy| password |NeverIndexMePassword|1|1",
                "field-token|credential-secondary| ToKeN |NeverIndexMeToken|1|0",
            ]
        || tags
            != [
                "bookmark-legacy|AI Tag|ai tag|ai",
                "credential-legacy|Manual Tag|manual tag|manual",
            ]
        || metadata
            != [
                "bookmark-legacy|approved safe summary|[\"validation portal\"]|bookmark-validation-hash|ready",
                "credential-legacy|pending private prompt output|[\"pending alias\"]|credential-validation-hash|pending",
            ]
    {
        return Err("fixture payload verification failed".into());
    }
    Ok(())
}

fn count(connection: &Connection, sql: &str) -> rusqlite::Result<i64> {
    connection.query_row(sql, [], |row| row.get(0))
}

fn strings(connection: &Connection, sql: &str) -> rusqlite::Result<Vec<String>> {
    connection
        .prepare(sql)?
        .query_map([], |row| row.get(0))?
        .collect()
}

fn memberships(connection: &Connection, table: &str) -> rusqlite::Result<Vec<(String, f64)>> {
    connection
        .prepare(&format!(
            "SELECT entry_id, sort_order FROM {table} ORDER BY entry_id"
        ))?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect()
}

fn id_counts(
    connection: &Connection,
    table: &str,
    id_column: &str,
) -> rusqlite::Result<Vec<(String, i64)>> {
    connection
        .prepare(&format!(
            "SELECT {id_column}, COUNT(*) FROM {table}
             GROUP BY {id_column} ORDER BY {id_column}"
        ))?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect()
}

type CatalogIdentity = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<f64>,
    Option<f64>,
);

fn catalog_identity(connection: &Connection) -> rusqlite::Result<Vec<CatalogIdentity>> {
    connection
        .prepare(
            "SELECT unified_id, source, source_id, kind, retention_state,
                    cleanup_at, inbox_position, saved_position
             FROM content_catalog ORDER BY unified_id",
        )?
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })?
        .collect()
}

fn expected_catalog_identity() -> Vec<CatalogIdentity> {
    vec![
        (
            "dock:dual-image",
            "dock",
            "dual-image",
            "image",
            "saved",
            None,
            Some(1.5),
            Some(2.5),
        ),
        (
            "dock:home-text",
            "dock",
            "home-text",
            "text",
            "temporary",
            Some("2026-08-03T08:00:00+00:00"),
            Some(3.0),
            None,
        ),
        (
            "dock:note-file",
            "dock",
            "note-file",
            "file",
            "saved",
            None,
            None,
            Some(0.5),
        ),
        (
            "vault:bookmark-legacy",
            "vault",
            "bookmark-legacy",
            "bookmark",
            "saved",
            None,
            None,
            Some(4.5),
        ),
        (
            "vault:credential-legacy",
            "vault",
            "credential-legacy",
            "credential",
            "temporary",
            Some("2026-08-17T12:00:00+00:00"),
            Some(4.0),
            None,
        ),
        (
            "vault:credential-secondary",
            "vault",
            "credential-secondary",
            "credential",
            "saved",
            None,
            None,
            Some(6.5),
        ),
        (
            "vault:note-legacy",
            "vault",
            "note-legacy",
            "note",
            "saved",
            None,
            None,
            Some(5.5),
        ),
    ]
    .into_iter()
    .map(
        |(id, source, source_id, kind, retention, cleanup, inbox, saved)| {
            (
                id.into(),
                source.into(),
                source_id.into(),
                kind.into(),
                retention.into(),
                cleanup.map(str::to_string),
                inbox,
                saved,
            )
        },
    )
    .collect()
}

fn open_fixture(path: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    let exclusive = OpenFlags::from_bits_retain(0x0000_0010);
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | exclusive,
    )?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "DELETE")?;
    Ok(connection)
}

fn create_legacy_fixture(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut connection = open_fixture(path)?;
    ensure_dock_schema(&mut connection)?;
    ensure_vault_schema(&mut connection)?;
    insert_fixed_payloads(&connection)?;
    connection.execute_batch("VACUUM;")?;
    Ok(())
}

fn create_fresh_fixture(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut connection = open_fixture(path)?;
    ensure_dock_schema(&mut connection)?;
    ensure_vault_schema(&mut connection)?;
    insert_fixed_payloads(&connection)?;
    ensure_content_schema(&mut connection, 30)?;
    connection.execute_batch(
        r#"
        UPDATE content_catalog
        SET retention_state = 'temporary',
            retention_changed_at = '2026-07-18T12:00:00+00:00',
            cleanup_at = '2026-08-17T12:00:00+00:00',
            inbox_position = 4.0,
            saved_position = NULL
        WHERE unified_id = 'vault:credential-legacy';
        UPDATE content_state SET revision = 9 WHERE singleton = 1;
        INSERT INTO content_pending_deletes(
            token, unified_id, created_at, expires_at, status
        ) VALUES (
            'validation-delete-token-000000000001',
            'dock:home-text',
            '2026-07-18T12:00:00+00:00',
            '2026-07-18T12:00:10+00:00',
            'pending'
        );
        "#,
    )?;
    connection.execute_batch("VACUUM;")?;
    Ok(())
}

fn insert_fixed_payloads(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        r#"
        INSERT INTO entries(
            id, kind, content, file_path, file_name, mime_type, width, height,
            size_bytes, collapsed, source, created_at, updated_at, title
        ) VALUES
            ('home-text', 'text', 'Validation home text', NULL, NULL, NULL, NULL, NULL,
             NULL, 0, 'validation', '2026-07-01T08:00:00+00:00',
             '2026-07-08T08:00:00+00:00', 'Home text'),
            ('note-file', 'file', NULL, 'fixtures/validation.pdf', 'validation.pdf',
             'application/pdf', NULL, NULL, 4096, 0, 'validation',
             '2026-07-02T08:00:00+00:00', '2026-07-07T08:00:00+00:00',
             'Validation file'),
            ('dual-image', 'image', NULL, 'fixtures/validation.png', 'validation.png',
             'image/png', 640, 480, 2048, 0, 'validation',
             '2026-07-03T08:00:00+00:00', '2026-07-06T08:00:00+00:00',
             'Validation image');

        INSERT INTO home_entries(entry_id, created_at, sort_order) VALUES
            ('home-text', '2026-07-04T08:00:00+00:00', 3.0),
            ('dual-image', '2026-07-04T09:00:00+00:00', 1.5);
        INSERT INTO note_entries(entry_id, created_at, sort_order) VALUES
            ('note-file', '2026-07-05T08:00:00+00:00', 0.5),
            ('dual-image', '2026-07-05T09:00:00+00:00', 2.5);

        INSERT INTO vault_entries(id, kind, title, notes, created_at, updated_at) VALUES
            ('credential-legacy', 'credential', 'Validation credential', 'safe notes',
             '2026-07-01T10:00:00+00:00', '2026-07-10T10:00:00+00:00'),
            ('bookmark-legacy', 'bookmark', 'Validation bookmark', 'bookmark notes',
             '2026-07-02T10:00:00+00:00', '2026-07-09T10:00:00+00:00'),
            ('note-legacy', 'note', 'Validation note', 'note body',
             '2026-07-03T10:00:00+00:00', '2026-07-08T10:00:00+00:00'),
            ('credential-secondary', 'credential', 'Secondary credential', NULL,
             '2026-07-04T10:00:00+00:00', '2026-07-07T10:00:00+00:00');

        INSERT INTO vault_fields(id, entry_id, key, value, is_sensitive, sort_order) VALUES
            ('field-user', 'credential-legacy', 'username', 'fixture-user', 0, 0),
            ('field-password', 'credential-legacy', ' password ',
             'NeverIndexMePassword', 1, 1),
            ('field-token', 'credential-secondary', ' ToKeN ',
             'NeverIndexMeToken', 1, 0),
            ('field-url', 'bookmark-legacy', 'url', 'https://fixture.invalid', 0, 0);

        INSERT INTO vault_tags(entry_id, tag, normalized_tag, source) VALUES
            ('credential-legacy', 'Manual Tag', 'manual tag', 'manual'),
            ('bookmark-legacy', 'AI Tag', 'ai tag', 'ai');
        INSERT INTO vault_ai_metadata(
            entry_id, summary, search_aliases_json, content_hash, status
        ) VALUES
            ('bookmark-legacy', 'approved safe summary', '["validation portal"]',
             'bookmark-validation-hash', 'ready'),
            ('credential-legacy', 'pending private prompt output', '["pending alias"]',
             'credential-validation-hash', 'pending');

        INSERT INTO vault_fts(entry_id, title, notes, searchable) VALUES
            ('credential-legacy', 'Validation credential', 'safe notes',
             'fixture-user Manual Tag'),
            ('bookmark-legacy', 'Validation bookmark', 'bookmark notes',
             'https://fixture.invalid AI Tag approved safe summary validation portal'),
            ('note-legacy', 'Validation note', 'note body', ''),
            ('credential-secondary', 'Secondary credential', '', '');
        "#,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::time::SystemTime;

    use super::*;

    fn temporary_directory(label: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "scratchpad-fixture-example-{label}-{}",
            rand::random::<u64>()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn fingerprint(path: &Path) -> (Vec<u8>, SystemTime) {
        (
            fs::read(path).unwrap(),
            fs::metadata(path).unwrap().modified().unwrap(),
        )
    }

    fn staging_count(parent: &Path) -> usize {
        fs::read_dir(parent)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(STAGING_PREFIX))
            })
            .count()
    }

    fn competitor_sentinel(parent: &Path) -> (PathBuf, (Vec<u8>, SystemTime)) {
        let competitor = parent.join("competitor");
        fs::create_dir(&competitor).unwrap();
        let sentinel = competitor.join("sentinel.txt");
        fs::write(&sentinel, b"competitor").unwrap();
        let before = fingerprint(&sentinel);
        (sentinel, before)
    }

    #[test]
    fn owner_open_failure_cleans_dir_only_staging_and_preserves_competitor() {
        let parent = temporary_directory("owner-open-failure");
        let (sentinel, before) = competitor_sentinel(&parent);

        assert!(StagingGuard::create_with_failure(&parent, OwnerFailure::BeforeOpen).is_err());

        assert_eq!(staging_count(&parent), 0);
        assert_eq!(fingerprint(&sentinel), before);
        fs::remove_file(sentinel).unwrap();
        fs::remove_dir(parent.join("competitor")).unwrap();
        fs::remove_dir(parent).unwrap();
    }

    #[test]
    fn owner_write_failure_removes_only_partial_owned_marker_and_staging() {
        let parent = temporary_directory("owner-write-failure");
        let (sentinel, before) = competitor_sentinel(&parent);

        assert!(StagingGuard::create_with_failure(&parent, OwnerFailure::DuringWrite).is_err());

        assert_eq!(staging_count(&parent), 0);
        assert_eq!(fingerprint(&sentinel), before);
        fs::remove_file(sentinel).unwrap();
        fs::remove_dir(parent.join("competitor")).unwrap();
        fs::remove_dir(parent).unwrap();
    }

    #[test]
    fn owner_remove_failure_prevents_publish_and_cleans_owned_staging() {
        let parent = temporary_directory("owner-remove-failure");
        let output = parent.join("output");

        assert!(generate_fixtures_with_failures(
            &output,
            OwnerFailure::None,
            PublishFailure::BeforeOwnerRemove,
            |_, _| Ok(())
        )
        .is_err());

        assert!(!output.exists());
        assert_eq!(staging_count(&parent), 0);
        fs::remove_dir(parent).unwrap();
    }

    #[test]
    fn successful_publish_contains_exactly_the_two_databases() {
        let parent = temporary_directory("exact-output");
        let output = parent.join("output");

        generate_fixtures(&output, |_, _| Ok(())).unwrap();

        let mut names = fs::read_dir(&output)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(
            names,
            [
                std::ffi::OsString::from(FRESH_NAME),
                std::ffi::OsString::from(LEGACY_NAME)
            ]
        );
        fs::remove_file(output.join(FRESH_NAME)).unwrap();
        fs::remove_file(output.join(LEGACY_NAME)).unwrap();
        fs::remove_dir(output).unwrap();
        fs::remove_dir(parent).unwrap();
    }

    #[test]
    fn failed_staging_cleanup_removes_owned_journal_but_not_unrelated_files() {
        let parent = temporary_directory("journal-cleanup");
        let output = parent.join("output");
        let unrelated = parent.join("keep-me.txt");
        fs::write(&unrelated, b"unrelated").unwrap();
        let unrelated_before = fingerprint(&unrelated);

        let result = generate_fixtures(&output, |staging, targets| {
            fs::write(
                format!("{}-journal", targets[1].display()),
                b"created during failed run",
            )?;
            assert!(staging.join(OWNER_FILE).is_file());
            Err("injected failure".into())
        });

        assert!(result.is_err());
        assert!(!output.exists());
        assert_eq!(staging_count(&parent), 0);
        assert_eq!(fingerprint(&unrelated), unrelated_before);
        fs::remove_file(unrelated).unwrap();
        fs::remove_dir(parent).unwrap();
    }

    #[test]
    fn publish_race_preserves_competing_output_and_cleans_owned_staging() {
        let parent = temporary_directory("publish-race");
        let output = parent.join("output");
        let mut sentinel_before = None;

        let result = generate_fixtures(&output, |_, _| {
            fs::create_dir(&output)?;
            let sentinel = output.join("competitor.txt");
            fs::write(&sentinel, b"competitor")?;
            sentinel_before = Some(fingerprint(&sentinel));
            Ok(())
        });

        assert!(result.is_err());
        assert_eq!(
            fingerprint(&output.join("competitor.txt")),
            sentinel_before.unwrap()
        );
        assert_eq!(staging_count(&parent), 0);
        fs::remove_file(output.join("competitor.txt")).unwrap();
        fs::remove_dir(output).unwrap();
        fs::remove_dir(parent).unwrap();
    }

    #[test]
    fn preexisting_output_symlink_is_rejected_without_touching_its_sentinel() {
        let parent = temporary_directory("symlink-output");
        let target = parent.join("competitor");
        let output = parent.join("output");
        fs::create_dir(&target).unwrap();
        let sentinel = target.join("sentinel.txt");
        fs::write(&sentinel, b"competitor").unwrap();
        let before = fingerprint(&sentinel);
        create_directory_symlink(&target, &output).unwrap();

        assert!(generate_fixtures(&output, |_, _| Ok(())).is_err());

        assert_eq!(fingerprint(&sentinel), before);
        assert_eq!(staging_count(&parent), 0);
        fs::remove_dir(output).unwrap();
        fs::remove_file(sentinel).unwrap();
        fs::remove_dir(target).unwrap();
        fs::remove_dir(parent).unwrap();
    }

    #[test]
    fn preexisting_output_directory_is_rejected_without_touching_its_sentinel() {
        let parent = temporary_directory("directory-output");
        let output = parent.join("output");
        fs::create_dir(&output).unwrap();
        let sentinel = output.join("sentinel.txt");
        fs::write(&sentinel, b"competitor").unwrap();
        let before = fingerprint(&sentinel);

        assert!(generate_fixtures(&output, |_, _| Ok(())).is_err());

        assert_eq!(fingerprint(&sentinel), before);
        assert_eq!(staging_count(&parent), 0);
        fs::remove_file(sentinel).unwrap();
        fs::remove_dir(output).unwrap();
        fs::remove_dir(parent).unwrap();
    }

    #[test]
    fn concurrent_publish_has_one_winner_and_loser_never_deletes_output() {
        let parent = temporary_directory("concurrent");
        let output = parent.join("output");
        let barrier = Arc::new(Barrier::new(2));
        let handles = (0..2)
            .map(|_| {
                let output = output.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    generate_fixtures(&output, |_, _| {
                        barrier.wait();
                        Ok(())
                    })
                    .is_ok()
                })
            })
            .collect::<Vec<_>>();
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(outcomes.iter().filter(|outcome| **outcome).count(), 1);
        assert!(verify_fresh_fixture(&output.join(FRESH_NAME)).is_ok());
        assert_eq!(staging_count(&parent), 0);
        fs::remove_dir_all(output).unwrap();
        fs::remove_dir(parent).unwrap();
    }

    #[test]
    fn fresh_verifier_rejects_an_extra_pending_delete_column() {
        let directory = temporary_directory("pending-schema");
        let database = directory.join(FRESH_NAME);
        create_fresh_fixture(&database).unwrap();
        let connection = Connection::open(&database).unwrap();
        connection
            .execute(
                "ALTER TABLE content_pending_deletes ADD COLUMN leaked TEXT",
                [],
            )
            .unwrap();
        drop(connection);

        assert!(verify_fresh_fixture(&database).is_err());

        fs::remove_file(database).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    fn mutate_fresh_and_verify_failure(label: &str, sql: &str) {
        let directory = temporary_directory(label);
        let database = directory.join(FRESH_NAME);
        create_fresh_fixture(&database).unwrap();
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch(sql).unwrap();
        drop(connection);
        assert!(verify_fresh_fixture(&database).is_err());
        fs::remove_file(database).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn verifier_rejects_payload_column_change_with_all_counts_unchanged() {
        mutate_fresh_and_verify_failure(
            "payload-column",
            "UPDATE entries SET file_name='changed.pdf' WHERE id='note-file'",
        );
    }

    #[test]
    fn verifier_rejects_vault_metadata_column_change_with_all_counts_unchanged() {
        mutate_fresh_and_verify_failure(
            "vault-metadata-column",
            "UPDATE vault_ai_metadata SET provider_id='changed-provider',
                 generated_at='2026-07-19T00:00:00+00:00'
             WHERE entry_id='bookmark-legacy'",
        );
    }

    #[test]
    fn verifier_rejects_catalog_timestamp_change_with_all_counts_unchanged() {
        mutate_fresh_and_verify_failure(
            "catalog-timestamp",
            "UPDATE content_catalog SET updated_at='2026-07-19T00:00:00+00:00'
             WHERE unified_id='dock:home-text'",
        );
    }

    #[test]
    fn verifier_rejects_vault_fts_text_change_with_all_counts_unchanged() {
        mutate_fresh_and_verify_failure(
            "vault-fts-text",
            "UPDATE vault_fts SET notes='changed safe notes'
             WHERE entry_id='credential-legacy'",
        );
    }

    #[test]
    fn verifier_rejects_content_fts_text_change_with_all_counts_unchanged() {
        mutate_fresh_and_verify_failure(
            "content-fts-text",
            "UPDATE content_fts SET title='Changed safe title'
             WHERE unified_id='dock:home-text'",
        );
    }

    #[test]
    fn verifier_rejects_catalog_identity_change_independently() {
        mutate_fresh_and_verify_failure(
            "catalog-identity",
            "UPDATE content_catalog SET source_id='wrong-source-id'
             WHERE unified_id='dock:home-text'",
        );
    }

    #[test]
    fn verifier_rejects_membership_change_independently() {
        mutate_fresh_and_verify_failure(
            "membership",
            "UPDATE home_entries SET sort_order=9.0 WHERE entry_id='home-text'",
        );
    }

    #[test]
    fn verifier_rejects_foreign_key_corruption_independently() {
        mutate_fresh_and_verify_failure(
            "foreign-key",
            "PRAGMA foreign_keys=OFF;
             UPDATE home_entries SET entry_id='missing-entry' WHERE entry_id='home-text'",
        );
    }
}

#[cfg(all(test, windows))]
fn create_directory_symlink(target: &Path, link: &Path) -> io::Result<()> {
    match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(1314) => {
            let status = std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .status()?;
            if status.success() {
                Ok(())
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(all(test, unix))]
fn create_directory_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}
