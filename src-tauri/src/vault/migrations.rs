// src-tauri/src/vault/migrations.rs
// Vault 独立 schema 版本管理 + v1→v2 迁移逻辑
use rusqlite::{params, Connection};

use crate::storage::error::{StorageError, StorageResult};

/// 标签归一化：trim → Unicode 小写 → 折叠内部连续空白为单空格；空串视为非法。
pub fn normalize_tag(raw: &str) -> Option<String> {
    let lower: String = raw.trim().to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_space = true; // 起始视为"刚结束空白"，避免前导空格
    for ch in lower.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    if prev_space && !out.is_empty() {
        // 去掉末尾单空格
        out.pop();
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 读取 vault schema 当前版本；版本表不存在时返回 0。
fn read_vault_version(conn: &Connection) -> StorageResult<u32> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='vault_schema_version'",
        [],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Ok(0);
    }
    let v: i64 = conn.query_row(
        "SELECT version FROM vault_schema_version WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    Ok(v as u32)
}

/// 写入版本号（接受 &Connection，事务和非事务场景都能用）。
fn write_vault_version(conn: &Connection, version: u32) -> StorageResult<()> {
    conn.execute(
        "INSERT INTO vault_schema_version(singleton, version) VALUES (1, ?1)
         ON CONFLICT(singleton) DO UPDATE SET version = excluded.version",
        params![version as i64],
    )?;
    Ok(())
}

/// 入口：将 vault schema 从当前版本升级到最新（v2）。
/// 调用方必须已确保 v1 表存在（即 storage::ensure_vault_schema 的 v1 建表 SQL 已执行）。
pub fn migrate_vault_schema(conn: &mut Connection) -> StorageResult<()> {
    // 全新数据库 / 旧版无版本表：先创建版本表并写入 version=1 作为基线
    let current = read_vault_version(conn)?;
    if current == 0 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS vault_schema_version (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                version INTEGER NOT NULL
            );",
        )?;
        write_vault_version(conn, 1)?;
    }
    let current = read_vault_version(conn)?;
    if current >= 2 {
        return Ok(());
    }

    // === v1 → v2 ===
    // 整个迁移在一个事务内完成，保证原子性。
    let tx = conn.transaction()?;

    // 1) 读出旧 vault_tags 到内存
    let legacy_tags: Vec<(String, String)> = {
        let mut stmt = tx.prepare("SELECT entry_id, tag FROM vault_tags")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut v = Vec::new();
        for row in rows {
            v.push(row?);
        }
        v
    };

    // 2) 创建 v2 临时表与新表
    tx.execute_batch(
        r#"
        CREATE TABLE vault_tags_v2 (
            entry_id TEXT NOT NULL REFERENCES vault_entries(id) ON DELETE CASCADE,
            tag TEXT NOT NULL,
            normalized_tag TEXT NOT NULL,
            source TEXT NOT NULL CHECK (source IN ('manual', 'ai')),
            PRIMARY KEY (entry_id, normalized_tag, source)
        );
        CREATE TABLE IF NOT EXISTS vault_ai_metadata (
            entry_id TEXT PRIMARY KEY REFERENCES vault_entries(id) ON DELETE CASCADE,
            summary TEXT,
            search_aliases_json TEXT NOT NULL DEFAULT '[]',
            content_hash TEXT NOT NULL,
            provider_id TEXT,
            model TEXT,
            generated_at TEXT,
            status TEXT NOT NULL CHECK (status IN ('ready', 'pending', 'error'))
        );
        CREATE TABLE IF NOT EXISTS vault_capture_requests (
            request_id TEXT PRIMARY KEY,
            entry_id TEXT NOT NULL REFERENCES vault_entries(id) ON DELETE CASCADE,
            created_at TEXT NOT NULL
        );
        "#,
    )?;

    // 3) 写入归一化后的标签（source=manual）
    //    tag 列保留 trim 后的原显示形式；normalized_tag 为归一化形式。
    {
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        for (entry_id, raw) in &legacy_tags {
            if let Some(norm) = normalize_tag(raw) {
                let key = (entry_id.clone(), norm.clone());
                // 同一 entry + 同一 normalized + 同一 source('manual')：去重一次
                if seen.insert(key) {
                    let display = raw.trim().to_string();
                    tx.execute(
                        "INSERT INTO vault_tags_v2(entry_id, tag, normalized_tag, source)
                         VALUES (?1, ?2, ?3, 'manual')",
                        params![entry_id, display, norm],
                    )?;
                }
            }
        }
    }

    // 4) 删除旧表，重命名 v2 表为 vault_tags
    tx.execute_batch(
        r#"
        DROP TABLE vault_tags;
        ALTER TABLE vault_tags_v2 RENAME TO vault_tags;
        CREATE INDEX IF NOT EXISTS idx_vault_tags_tag ON vault_tags(tag);
        CREATE INDEX IF NOT EXISTS idx_vault_tags_normalized ON vault_tags(normalized_tag);
        "#,
    )?;

    // 5) 重建 FTS 索引（vault_fts 已由 v1 SQL 创建；先清空，再从 entry+非敏感 field+tag 重建）
    tx.execute("DELETE FROM vault_fts", [])?;
    {
        let entries: Vec<(String, String, Option<String>)> = {
            let mut stmt =
                tx.prepare("SELECT id, title, notes FROM vault_entries ORDER BY created_at")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?;
            let mut v = Vec::new();
            for row in rows {
                v.push(row?);
            }
            v
        };
        for (id, title, notes) in entries {
            let searchable = rebuild_searchable(&tx, &id)?;
            tx.execute(
                "INSERT INTO vault_fts(entry_id, title, notes, searchable)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, title, notes.unwrap_or_default(), searchable],
            )?;
        }
    }

    // 6) 写入 version=2
    write_vault_version(&tx, 2)
        .map_err(|e| StorageError::Migration(format!("write version 2: {e}")))?;

    tx.commit()?;
    Ok(())
}

/// 仅用于迁移：从 entry、非敏感 field、tag 构建可搜索文本
/// （迁移路径专用，避免引用 storage 模块私有函数）。
fn rebuild_searchable(conn: &Connection, entry_id: &str) -> StorageResult<String> {
    let mut parts: Vec<String> = Vec::new();
    {
        let mut stmt =
            conn.prepare("SELECT value FROM vault_fields WHERE entry_id=?1 AND is_sensitive=0")?;
        let rows = stmt.query_map(params![entry_id], |r| r.get::<_, String>(0))?;
        for r in rows {
            parts.push(r?);
        }
    }
    {
        let mut stmt = conn.prepare("SELECT tag FROM vault_tags WHERE entry_id=?1")?;
        let rows = stmt.query_map(params![entry_id], |r| r.get::<_, String>(0))?;
        for r in rows {
            parts.push(r?);
        }
    }
    Ok(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::storage::ensure_vault_schema;
    use rusqlite::Connection;

    /// 模拟 v2 之前的"旧版" vault 数据库（包含旧 vault_tags 而无 normalized/source 列）。
    fn seed_legacy_vault(conn: &mut Connection) {
        conn.execute_batch(
            r#"
        CREATE TABLE vault_entries (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            notes TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE vault_fields (
            id TEXT PRIMARY KEY,
            entry_id TEXT NOT NULL REFERENCES vault_entries(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            is_sensitive INTEGER NOT NULL DEFAULT 0,
            sort_order INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE vault_tags (
            entry_id TEXT NOT NULL REFERENCES vault_entries(id) ON DELETE CASCADE,
            tag TEXT NOT NULL,
            PRIMARY KEY (entry_id, tag)
        );
        CREATE VIRTUAL TABLE vault_fts USING fts5(
            entry_id UNINDEXED,
            title,
            notes,
            searchable,
            tokenize = 'unicode61'
        );
        "#,
        )
        .unwrap();
    }

    fn insert_legacy_entry(conn: &mut Connection, id: &str, title: &str) {
        conn.execute(
            "INSERT INTO vault_entries(id, kind, title, created_at, updated_at)
             VALUES (?1, 'credential', ?2, '2026-07-01T00:00:00Z', '2026-07-01T00:00:00Z')",
            params![id, title],
        )
        .unwrap();
    }

    #[test]
    fn migration_preserves_legacy_tags_as_manual() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        seed_legacy_vault(&mut conn);
        conn.execute(
            "INSERT INTO vault_entries(id, kind, title, created_at, updated_at)
             VALUES ('v1', 'credential', 'DB', '2026-07-01T00:00:00Z', '2026-07-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vault_tags(entry_id, tag) VALUES ('v1', '  Production  ')",
            [],
        )
        .unwrap();

        ensure_vault_schema(&mut conn).unwrap();

        let row: (String, String, String) = conn
            .query_row(
                "SELECT tag, normalized_tag, source FROM vault_tags WHERE entry_id='v1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            ("Production".into(), "production".into(), "manual".into())
        );
    }

    #[test]
    fn migration_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        seed_legacy_vault(&mut conn);
        insert_legacy_entry(&mut conn, "v1", "E1");
        conn.execute(
            "INSERT INTO vault_tags(entry_id, tag) VALUES ('v1', 'alpha')",
            [],
        )
        .unwrap();

        ensure_vault_schema(&mut conn).unwrap();
        // 再跑一次：ensure_vault_schema 应该幂等（版本已=2，跳过）
        ensure_vault_schema(&mut conn).unwrap();

        let v: i64 = conn
            .query_row(
                "SELECT version FROM vault_schema_version WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, 2);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vault_tags WHERE entry_id='v1' AND source='manual'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn migration_creates_ai_metadata_and_capture_request_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        seed_legacy_vault(&mut conn);

        ensure_vault_schema(&mut conn).unwrap();

        for table in ["vault_ai_metadata", "vault_capture_requests"] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
                    params![table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing table {table}");
        }
    }

    #[test]
    fn migration_allows_same_normalized_tag_from_manual_and_ai() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        seed_legacy_vault(&mut conn);
        insert_legacy_entry(&mut conn, "v1", "E1");

        ensure_vault_schema(&mut conn).unwrap();

        // 迁移后：手动插入 manual + ai 同名标签
        conn.execute(
            "INSERT INTO vault_tags(entry_id, tag, normalized_tag, source)
             VALUES ('v1', 'prod', 'prod', 'manual')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vault_tags(entry_id, tag, normalized_tag, source)
             VALUES ('v1', 'Prod', 'prod', 'ai')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vault_tags WHERE entry_id='v1' AND normalized_tag='prod'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn migration_rejects_duplicate_tag_for_same_source() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        seed_legacy_vault(&mut conn);
        insert_legacy_entry(&mut conn, "v1", "E1");

        ensure_vault_schema(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO vault_tags(entry_id, tag, normalized_tag, source)
             VALUES ('v1', 'prod', 'prod', 'manual')",
            [],
        )
        .unwrap();
        // 同 (entry_id, normalized_tag, source) 应被主键约束拒绝
        let dup = conn.execute(
            "INSERT INTO vault_tags(entry_id, tag, normalized_tag, source)
             VALUES ('v1', 'Prod', 'prod', 'manual')",
            [],
        );
        assert!(dup.is_err());
    }

    #[test]
    fn normalize_tag_trims_lowercases_and_collapses_whitespace() {
        assert_eq!(
            normalize_tag("  Production  ").as_deref(),
            Some("production")
        );
        assert_eq!(
            normalize_tag("Foo   Bar\tBaz").as_deref(),
            Some("foo bar baz")
        );
        assert_eq!(normalize_tag("MIXEDCase").as_deref(), Some("mixedcase"));
        assert_eq!(normalize_tag("   ").as_deref(), None);
        assert_eq!(normalize_tag("").as_deref(), None);
    }
}
