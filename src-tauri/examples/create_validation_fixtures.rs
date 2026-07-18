use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use soma_scratchpad::content::migrations::ensure_content_schema;
use soma_scratchpad::scratchpad::storage::ensure_dock_schema;
use soma_scratchpad::vault::storage::ensure_vault_schema;

const LEGACY_NAME: &str = "legacy-validation.sqlite3";
const FRESH_NAME: &str = "fresh-validation.sqlite3";

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
    if let Ok(metadata) = fs::symlink_metadata(&output_directory) {
        if metadata.file_type().is_symlink() {
            return Err("output directory must not be a symlink".into());
        }
        if !metadata.is_dir() {
            return Err("output path is not a directory".into());
        }
    }

    let targets = [
        output_directory.join(LEGACY_NAME),
        output_directory.join(FRESH_NAME),
    ];
    for target in &targets {
        reject_existing_target(target)?;
    }

    let made_output_directory = !output_directory.exists();
    if made_output_directory {
        fs::create_dir_all(&output_directory)?;
    }

    let result = (|| {
        create_legacy_fixture(&targets[0])?;
        create_fresh_fixture(&targets[1])?;
        let legacy = verify_legacy_fixture(&targets[0])?;
        let fresh = verify_fresh_fixture(&targets[1])?;
        Ok::<_, Box<dyn std::error::Error>>((legacy, fresh))
    })();
    let (legacy, fresh) = match result {
        Ok(verification) => verification,
        Err(error) => {
            cleanup_created_targets(&targets);
            if made_output_directory {
                let _ = fs::remove_dir(&output_directory);
            }
            return Err(error);
        }
    };

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

fn verify_legacy_fixture(path: &Path) -> Result<(i64, i64, i64), Box<dyn std::error::Error>> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
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
    let content_schema: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name='content_catalog'",
        [],
        |row| row.get(0),
    )?;
    if (main_version, vault_version, payloads, content_schema) != (2, 4, 7, 0) {
        return Err("legacy fixture verification failed".into());
    }
    Ok((main_version, vault_version, payloads))
}

type FreshVerification = (i64, i64, i64, i64, i64, i64, i64, i64, i64);

fn verify_fresh_fixture(path: &Path) -> Result<FreshVerification, Box<dyn std::error::Error>> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
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
    let pending = count(
        &connection,
        "SELECT COUNT(*) FROM content_pending_deletes
         WHERE token='validation-delete-token-000000000001'
           AND unified_id='dock:home-text'
           AND created_at='2026-07-18T12:00:00+00:00'
           AND expires_at='2026-07-18T12:00:10+00:00'
           AND status='pending'",
    )?;
    let unsafe_fts = count(
        &connection,
        "SELECT
            (SELECT COUNT(*) FROM content_fts
             WHERE lower(title || body || tags || aliases) LIKE '%neverindexme%') +
            (SELECT COUNT(*) FROM vault_fts
             WHERE lower(title || notes || searchable) LIKE '%neverindexme%')",
    )?;
    let pending_columns = count(
        &connection,
        "SELECT COUNT(*) FROM pragma_table_info('content_pending_deletes')
         WHERE name IN ('token', 'unified_id', 'created_at', 'expires_at', 'status')",
    )?;
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
        pending_columns,
    ) != (4, 4, 7, 2, 5, 7, 4, 9, 1, 0, 5)
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

fn count(connection: &Connection, sql: &str) -> rusqlite::Result<i64> {
    connection.query_row(sql, [], |row| row.get(0))
}

fn reject_existing_target(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for candidate in sidecar_paths(path) {
        match fs::symlink_metadata(&candidate) {
            Ok(_) => {
                return Err(format!(
                    "refusing to overwrite existing fixture or sidecar: {}",
                    candidate.display()
                )
                .into())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn cleanup_created_targets(targets: &[PathBuf; 2]) {
    for target in targets {
        for path in sidecar_paths(target) {
            if fs::symlink_metadata(&path)
                .map(|metadata| metadata.file_type().is_file())
                .unwrap_or(false)
            {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn sidecar_paths(database: &Path) -> [PathBuf; 4] {
    [
        database.to_path_buf(),
        PathBuf::from(format!("{}-wal", database.display())),
        PathBuf::from(format!("{}-shm", database.display())),
        PathBuf::from(format!("{}-journal", database.display())),
    ]
}

fn open_fixture(path: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    let connection = Connection::open(path)?;
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

    #[test]
    fn preflight_rejects_either_journal_without_modifying_existing_files() {
        for database_name in [LEGACY_NAME, FRESH_NAME] {
            let directory = temporary_directory("journal-preflight");
            let journal = PathBuf::from(format!(
                "{}-journal",
                directory.join(database_name).display()
            ));
            let unrelated = directory.join("keep-me.txt");
            fs::write(&journal, b"existing journal").unwrap();
            fs::write(&unrelated, b"unrelated").unwrap();
            let before = [fingerprint(&journal), fingerprint(&unrelated)];

            assert!(reject_existing_target(&directory.join(database_name)).is_err());

            assert_eq!([fingerprint(&journal), fingerprint(&unrelated)], before);
            fs::remove_file(journal).unwrap();
            fs::remove_file(unrelated).unwrap();
            fs::remove_dir(directory).unwrap();
        }
    }

    #[test]
    fn failed_creation_cleanup_removes_all_database_files_but_not_unrelated_files() {
        let directory = temporary_directory("journal-cleanup");
        let targets = [directory.join(LEGACY_NAME), directory.join(FRESH_NAME)];
        let unrelated = directory.join("keep-me.txt");
        fs::write(&unrelated, b"unrelated").unwrap();
        let unrelated_before = fingerprint(&unrelated);
        for target in &targets {
            for path in [
                target.to_path_buf(),
                PathBuf::from(format!("{}-wal", target.display())),
                PathBuf::from(format!("{}-shm", target.display())),
                PathBuf::from(format!("{}-journal", target.display())),
            ] {
                fs::write(path, b"created during failed run").unwrap();
            }
        }

        cleanup_created_targets(&targets);

        for target in &targets {
            for path in [
                target.to_path_buf(),
                PathBuf::from(format!("{}-wal", target.display())),
                PathBuf::from(format!("{}-shm", target.display())),
                PathBuf::from(format!("{}-journal", target.display())),
            ] {
                assert!(!path.exists(), "cleanup left {}", path.display());
            }
        }
        assert_eq!(fingerprint(&unrelated), unrelated_before);
        fs::remove_file(unrelated).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
