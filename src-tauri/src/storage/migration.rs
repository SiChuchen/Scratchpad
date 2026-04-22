use rusqlite::{params, Connection, OptionalExtension};

use super::error::{StorageError, StorageResult};

const INIT_SCHEMA_VERSION_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    scope TEXT PRIMARY KEY CHECK (scope = 'main'),
    version INTEGER NOT NULL
);

INSERT OR IGNORE INTO schema_version(scope, version)
VALUES ('main', 0);
"#;

#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

impl Migration {
    pub const fn new(version: i64, name: &'static str, sql: &'static str) -> Self {
        Self { version, name, sql }
    }
}

pub fn ensure_schema(conn: &mut Connection, migrations: &[Migration]) -> StorageResult<()> {
    ensure_version_table(conn)?;

    let current_version = get_schema_version(conn)?;
    let mut ordered = migrations.to_vec();
    ordered.sort_by_key(|migration| migration.version);

    validate_migrations(&ordered)?;

    let mut expected_version = current_version;
    for migration in ordered {
        if migration.version <= current_version {
            continue;
        }

        if migration.version != expected_version + 1 {
            return Err(StorageError::Migration(format!(
                "missing migration between versions {expected_version} and {}",
                migration.version
            )));
        }

        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        set_schema_version(&tx, migration.version)?;
        tx.commit()?;
        expected_version = migration.version;
    }

    Ok(())
}

pub fn get_schema_version(conn: &Connection) -> StorageResult<i64> {
    ensure_version_table(conn)?;

    let version = conn
        .query_row(
            "SELECT version FROM schema_version WHERE scope = 'main'",
            [],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);

    Ok(version)
}

pub fn set_schema_version(conn: &Connection, version: i64) -> StorageResult<()> {
    ensure_version_table(conn)?;
    conn.execute(
        r#"
        INSERT INTO schema_version(scope, version)
        VALUES ('main', ?1)
        ON CONFLICT(scope) DO UPDATE SET version = excluded.version
        "#,
        params![version],
    )?;

    Ok(())
}

fn ensure_version_table(conn: &Connection) -> StorageResult<()> {
    conn.execute_batch(INIT_SCHEMA_VERSION_SQL)?;
    Ok(())
}

fn validate_migrations(migrations: &[Migration]) -> StorageResult<()> {
    for pair in migrations.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        if left.version == right.version {
            return Err(StorageError::Migration(format!(
                "duplicate migration version {} ({}, {})",
                left.version, left.name, right.name
            )));
        }
    }

    Ok(())
}
