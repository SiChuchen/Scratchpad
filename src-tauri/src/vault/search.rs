// src-tauri/src/vault/search.rs
//
// Task 9: 本地混合检索 + 评分。
//
// `search_local` 把一个查询字符串（可选叠加 AI 查询计划）变成一队按
// 相关度排序的 `VaultSearchHit`。所有 SQL 都使用 rusqlite 的 `params!`
// 绑定参数，绝不字符串拼接用户文本。
//
// 评分规则（见 Task 9 plan）：
//   - 原查询 title 完整/子串：100
//   - 原查询 FTS/字段/tag：80
//   - AI keyword：55
//   - AI alias：45
//   - 同一条目多次命中 → 取最高基础分 + min(15, (extra_terms × 3))
//   - kind/date：硬过滤，不加分
//   - 仅当 metadata.status='ready' 且 content_hash 与当前 entry hash 一致时
//     才参与 AI term 搜索（防 stale metadata 误召回）
//
// CJK 兜底：unicode61 分词器对中文子串召回不稳定，所以我们在拼接
// `title || ' ' || notes || ' ' || searchable` 上额外做 LIKE 子串扫描，
// 专门给"无空格中文标题"找回来。

use std::collections::HashMap;

use rusqlite::{params, Connection};

use crate::storage::error::{StorageError, StorageResult};
use crate::vault::models::{
    AiMetadataStatus, AiQueryPlan, EntryKind, SearchSource, VaultEntry, VaultEntrySummary,
    VaultField, VaultSearchHit,
};
use crate::vault::storage as vstore;

// ---- 评分常量 -------------------------------------------------------------

const SCORE_TITLE_MATCH: f64 = 100.0;
const SCORE_FTS_OR_FIELD_OR_TAG: f64 = 80.0;
const SCORE_AI_KEYWORD: f64 = 55.0;
const SCORE_AI_ALIAS: f64 = 45.0;
/// 每个额外匹配 term 给基础分加多少（最多 `MAX_BONUS`）
const BONUS_PER_EXTRA_TERM: f64 = 3.0;
const MAX_BONUS: f64 = 15.0;

// ---- 聚合器 ---------------------------------------------------------------

/// 在内存中聚合一个 entry 的多条候选匹配 → 最终分数。
#[derive(Default)]
struct ScoreAccumulator {
    /// 已记录的最高 base score
    max_base: Option<f64>,
    /// 已经计入的 term 数（去重 key，用于额外加分）
    matched_terms: Vec<String>,
    /// 来源（Local / AiExpanded）
    sources: Vec<SearchSource>,
}

impl ScoreAccumulator {
    fn record(&mut self, term_key: &str, base: f64, source: SearchSource) {
        if !self.matched_terms.iter().any(|t| t == term_key) {
            self.matched_terms.push(term_key.to_string());
        }
        self.max_base = Some(self.max_base.map_or(base, |cur| cur.max(base)));
        if !self.sources.contains(&source) {
            self.sources.push(source);
        }
    }

    fn finalize_score(&self) -> f64 {
        let base = self.max_base.unwrap_or(0.0);
        // 额外 term 数 = 总 term 数 - 1（第一条不算 "额外"）
        let extra = self.matched_terms.len().saturating_sub(1) as f64;
        let bonus = (extra * BONUS_PER_EXTRA_TERM).min(MAX_BONUS);
        base + bonus
    }
}

// ---- 顶层入口 -------------------------------------------------------------

/// 顶层搜索入口：
///
/// - `query` 用户原始查询（trim 后若为空直接返回空数组）
/// - `plan` 可选的 AI 查询计划（keywords/aliases 用于扩展；kinds/dates 作为硬过滤）
/// - `limit` 自动 clamp 到 [1, 100]
///
/// 所有 SQL 使用参数化绑定。返回按分数降序排序的 `VaultSearchHit` 列表。
pub fn search_local(
    conn: &Connection,
    query: &str,
    plan: Option<&AiQueryPlan>,
    limit: usize,
) -> StorageResult<Vec<VaultSearchHit>> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.clamp(1, 100);

    let plan = plan.cloned().unwrap_or_default();

    // 1. 收集每个 entry 的候选匹配（entry_id → accumulator）
    let mut acc: HashMap<String, ScoreAccumulator> = HashMap::new();

    // -- 1a. 原查询：title 精确 / 子串 (100)
    add_title_matches(conn, trimmed, &mut acc, SearchSource::Local)?;

    // -- 1b. 原查询：CJK 子串兜底（也算 100，作为 title 子串的延伸，
    //         用于"无空格中文标题"场景；term key 用 "substr:<query>" 去重）
    add_cjk_substring_matches(conn, trimmed, &mut acc, SearchSource::Local)?;

    // -- 1c. 原查询：FTS match (80)
    add_fts_matches(conn, trimmed, &mut acc, SearchSource::Local)?;

    // -- 1d. 原查询：非敏感字段 value LIKE (80)
    add_field_matches(conn, trimmed, &mut acc, SearchSource::Local)?;

    // -- 1e. 原查询：tag LIKE (80)
    add_tag_matches(conn, trimmed, &mut acc, SearchSource::Local)?;

    // 2. AI 扩展（仅 ready + hash 匹配的 metadata 才参与）
    if !plan.keywords.is_empty() || !plan.aliases.is_empty() {
        add_ai_expansion(conn, &plan, &mut acc)?;
    }

    // 3. 应用 kind / date 硬过滤；同时算最终分
    let mut scored: Vec<(String, f64, ScoreAccumulator)> = Vec::with_capacity(acc.len());
    for (entry_id, accumulator) in acc.into_iter() {
        if !passes_hard_filters(conn, &entry_id, &plan)? {
            continue;
        }
        let score = accumulator.finalize_score();
        scored.push((entry_id, score, accumulator));
    }

    // 4. 按分数降序，再按 updated_at 降序做次序 tiebreaker
    //    为了不在 sort_by 内查 DB，先取出 updated_at，外部排序。
    let mut with_time: Vec<(String, f64, ScoreAccumulator, String)> =
        Vec::with_capacity(scored.len());
    for (entry_id, score, accumulator) in scored.into_iter() {
        let updated_at = vstore::get_entry_by_id(conn, &entry_id)?
            .map(|e| e.updated_at)
            .unwrap_or_default();
        with_time.push((entry_id, score, accumulator, updated_at));
    }
    with_time.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.3.cmp(&a.3))
    });

    // 5. 拼装 VaultSearchHit；分页用 limit
    let mut hits: Vec<VaultSearchHit> = Vec::with_capacity(with_time.len());
    for (entry_id, score, accumulator, _updated_at) in with_time.into_iter().take(limit) {
        let summary = build_summary(conn, &entry_id)?;
        hits.push(VaultSearchHit {
            summary,
            score,
            sources: if accumulator.sources.is_empty() {
                vec![SearchSource::Local]
            } else {
                accumulator.sources
            },
        });
    }

    Ok(hits)
}

// ---- 子函数 ---------------------------------------------------------------

/// 标题精确 + 子串：都给 100 分；用同一个 term key "title" 防止重复加分。
fn add_title_matches(
    conn: &Connection,
    query: &str,
    acc: &mut HashMap<String, ScoreAccumulator>,
    source: SearchSource,
) -> StorageResult<()> {
    let like = format!("%{query}%");
    let mut stmt = conn.prepare(
        "SELECT id FROM vault_entries WHERE title = ?1 OR title LIKE ?2",
    )?;
    let ids: Vec<String> = stmt
        .query_map(params![query, like], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    for id in ids {
        acc.entry(id).or_default().record("title", SCORE_TITLE_MATCH, source);
    }
    Ok(())
}

/// CJK 子串兜底：扫描 `title || ' ' || notes || ' ' || searchable`。
/// 即便 FTS5 unicode61 分不出"数据库"这种连续 token，这里也能召回。
fn add_cjk_substring_matches(
    conn: &Connection,
    query: &str,
    acc: &mut HashMap<String, ScoreAccumulator>,
    source: SearchSource,
) -> StorageResult<()> {
    let like = format!("%{query}%");
    let mut stmt = conn.prepare(
        "SELECT entry_id FROM vault_fts
         WHERE (title || ' ' || notes || ' ' || searchable) LIKE ?1",
    )?;
    let ids: Vec<String> = stmt
        .query_map(params![like], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    for id in ids {
        // term key 用 "substr:<query>"：避免和 FTS/field/tag 那些重复加分。
        acc.entry(id)
            .or_default()
            .record(&format!("substr:{query}"), SCORE_TITLE_MATCH, source);
    }
    Ok(())
}

/// FTS5 MATCH 查询：复用 storage::fts5_search 的 escape 逻辑。
fn add_fts_matches(
    conn: &Connection,
    query: &str,
    acc: &mut HashMap<String, ScoreAccumulator>,
    source: SearchSource,
) -> StorageResult<()> {
    let fts_hits = vstore::fts5_search(conn, query, 200)?;
    for (id, _rank) in fts_hits {
        acc.entry(id)
            .or_default()
            .record("fts", SCORE_FTS_OR_FIELD_OR_TAG, source);
    }
    Ok(())
}

/// 非敏感字段 value LIKE 查询：80 分。
fn add_field_matches(
    conn: &Connection,
    query: &str,
    acc: &mut HashMap<String, ScoreAccumulator>,
    source: SearchSource,
) -> StorageResult<()> {
    let like = format!("%{query}%");
    let mut stmt = conn.prepare(
        "SELECT DISTINCT entry_id FROM vault_fields
         WHERE is_sensitive = 0 AND value LIKE ?1",
    )?;
    let ids: Vec<String> = stmt
        .query_map(params![like], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    for id in ids {
        acc.entry(id)
            .or_default()
            .record("field", SCORE_FTS_OR_FIELD_OR_TAG, source);
    }
    Ok(())
}

/// tag LIKE 查询（含 tag 和 normalized_tag）：80 分。
fn add_tag_matches(
    conn: &Connection,
    query: &str,
    acc: &mut HashMap<String, ScoreAccumulator>,
    source: SearchSource,
) -> StorageResult<()> {
    let like = format!("%{query}%");
    let mut stmt = conn.prepare(
        "SELECT DISTINCT entry_id FROM vault_tags
         WHERE tag LIKE ?1 OR normalized_tag LIKE ?1",
    )?;
    let ids: Vec<String> = stmt
        .query_map(params![like], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    for id in ids {
        acc.entry(id)
            .or_default()
            .record("tag", SCORE_FTS_OR_FIELD_OR_TAG, source);
    }
    Ok(())
}

/// AI keyword / alias 扩展：
/// - keyword 匹配：用 keyword 在 title/notes/searchable 上做 LIKE，55 分
/// - alias 匹配：在 vault_ai_metadata.search_aliases_json 中找包含 alias 的 ready 行，45 分
///   两者都必须满足 metadata.status='ready' 且 content_hash == entry 当前 hash。
fn add_ai_expansion(
    conn: &Connection,
    plan: &AiQueryPlan,
    acc: &mut HashMap<String, ScoreAccumulator>,
) -> StorageResult<()> {
    // 1) keyword: 在 vault_fts (title/notes/searchable) 做 LIKE；每个 keyword 独立计分。
    for kw in &plan.keywords {
        let trimmed = kw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let like = format!("%{trimmed}%");
        let mut stmt = conn.prepare(
            "SELECT entry_id FROM vault_fts
             WHERE (title || ' ' || notes || ' ' || searchable) LIKE ?1",
        )?;
        let ids: Vec<String> = stmt
            .query_map(params![like], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        for id in ids {
            // 仅在 ready+hash 匹配的 entry 上参与
            if metadata_is_ready_and_fresh(conn, &id)? {
                acc.entry(id).or_default().record(
                    &format!("ai-kw:{trimmed}"),
                    SCORE_AI_KEYWORD,
                    SearchSource::AiExpanded,
                );
            }
        }
    }

    // 2) alias: 在 vault_ai_metadata 中查找 alias 匹配；仅 ready+fresh 参与计算。
    //    SQLite 的 json_each 可以展开 search_aliases_json。
    for alias in &plan.aliases {
        let trimmed = alias.trim();
        if trimmed.is_empty() {
            continue;
        }
        let like = format!("%{trimmed}%");
        let mut stmt = conn.prepare(
            "SELECT m.entry_id
             FROM vault_ai_metadata m
             JOIN json_each(m.search_aliases_json) AS j
             WHERE m.status = 'ready'
               AND m.search_aliases_json LIKE ?1
               AND j.value LIKE ?1",
        )?;
        let ids: Vec<String> = stmt
            .query_map(params![like], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        for id in ids {
            if metadata_is_ready_and_fresh(conn, &id)? {
                acc.entry(id).or_default().record(
                    &format!("ai-alias:{trimmed}"),
                    SCORE_AI_ALIAS,
                    SearchSource::AiExpanded,
                );
            }
        }
    }

    Ok(())
}

/// 判断某 entry 的 metadata 是否 ready 且 content_hash 等于当前 entry hash。
/// content_hash 由 create_entry / update_entry 在写入时算好存在 metadata 行里，
/// 这里把存储的 metadata.content_hash 与当前 entry 的 fields/title/notes 重算
/// 出的 hash 比较；不等则视为 stale，不参与 AI 扩展。
fn metadata_is_ready_and_fresh(conn: &Connection, entry_id: &str) -> StorageResult<bool> {
    let metadata = match vstore::get_ai_metadata(conn, entry_id)? {
        Some(m) => m,
        None => return Ok(false),
    };
    if metadata.status != AiMetadataStatus::Ready {
        return Ok(false);
    }
    // 用当前 entry + fields 重算 hash，与 metadata.content_hash 比较。
    // 这样即便有人直接改 SQL 绕过 update_entry 流程把 metadata 标成 ready，
    // 只要内容不匹配就不会参与 AI 扩展。
    let entry = match vstore::get_entry_by_id(conn, entry_id)? {
        Some(e) => e,
        None => return Ok(false),
    };
    let fields = vstore::list_fields(conn, entry_id)?;
    let current_hash = vstore::compute_entry_content_hash(&entry, &fields);
    Ok(current_hash == metadata.content_hash)
}

/// 硬过滤：kind / date。任一不满足就丢掉该 entry。
fn passes_hard_filters(
    conn: &Connection,
    entry_id: &str,
    plan: &AiQueryPlan,
) -> StorageResult<bool> {
    let entry: Option<VaultEntry> = vstore::get_entry_by_id(conn, entry_id)?;
    let Some(entry) = entry else {
        return Ok(false);
    };

    // kind 过滤
    if !plan.kinds.is_empty() && !plan.kinds.contains(&entry.kind) {
        return Ok(false);
    }

    // date 过滤：用 created_at（YYYY-MM-DD 前缀）比较
    if let Some(from) = plan.date_from.as_deref() {
        if !date_gte(&entry.created_at, from) {
            return Ok(false);
        }
    }
    if let Some(to) = plan.date_to.as_deref() {
        if !date_lte(&entry.created_at, to) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// created_at >= from（用 YYYY-MM-DD 字符串比较即可）
fn date_gte(created_at: &str, from: &str) -> bool {
    let created_day = day_prefix(created_at);
    created_day >= from
}

/// created_at <= to
fn date_lte(created_at: &str, to: &str) -> bool {
    let created_day = day_prefix(created_at);
    created_day <= to
}

fn day_prefix(s: &str) -> &str {
    &s[..10.min(s.len())]
}

/// 拼装 VaultEntrySummary（含 tags 和 preview）
fn build_summary(conn: &Connection, entry_id: &str) -> StorageResult<VaultEntrySummary> {
    let entry = vstore::get_entry_by_id(conn, entry_id)?
        .ok_or_else(|| StorageError::Other(format!("entry not found: {entry_id}")))?;
    let tags = vstore::list_tags_with_source(conn, entry_id).unwrap_or_default();
    let fields: Vec<VaultField> = vstore::list_fields(conn, entry_id).unwrap_or_default();
    let preview = build_preview(&entry, &fields);
    Ok(VaultEntrySummary { entry, tags, preview })
}

/// 与 storage::build_preview 等价的预览生成（storage 里的是私有函数）。
/// 这里做最小可用版本：credential 取前 2 个非敏感字段 value，bookmark 取
/// url 字段，note 取 notes。统一 Unicode 截断到 120 字符。
fn build_preview(entry: &VaultEntry, fields: &[VaultField]) -> Option<String> {
    const MAX_LEN: usize = 120;
    let trim = |s: &str| -> Option<String> {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(unicode_truncate(t, MAX_LEN).to_string())
        }
    };
    let candidate: Option<String> = match entry.kind {
        EntryKind::Credential => {
            let non_sensitive: Vec<&VaultField> =
                fields.iter().filter(|f| !f.is_sensitive).take(2).collect();
            if non_sensitive.is_empty() {
                entry.notes.as_deref().and_then(&trim)
            } else {
                let joined = non_sensitive
                    .iter()
                    .map(|f| f.value.trim())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                if joined.is_empty() {
                    entry.notes.as_deref().and_then(&trim)
                } else {
                    trim(&joined)
                }
            }
        }
        EntryKind::Bookmark => {
            let url = fields
                .iter()
                .find(|f| f.key.eq_ignore_ascii_case("url") && !f.is_sensitive)
                .map(|f| f.value.trim())
                .filter(|s| !s.is_empty());
            url.and_then(trim)
                .or_else(|| entry.notes.as_deref().and_then(&trim))
        }
        EntryKind::Note => entry.notes.as_deref().and_then(&trim),
    };
    candidate
}

fn unicode_truncate(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let end = s
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    &s[..end]
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::models::{FieldInput, VaultAiMetadata, VaultEntryInput};

    fn open_test_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        vstore::ensure_vault_schema(&mut conn).unwrap();
        conn
    }

    fn input(title: &str) -> VaultEntryInput {
        VaultEntryInput {
            kind: EntryKind::Credential,
            title: title.into(),
            fields: vec![],
            notes: None,
            manual_tags: vec![],
        }
    }

    fn set_ready_metadata(conn: &mut Connection, entry_id: &str, aliases: &[&str]) {
        let metadata = VaultAiMetadata {
            entry_id: entry_id.into(),
            summary: Some("production database".into()),
            search_aliases: aliases.iter().map(|value| (*value).to_string()).collect(),
            content_hash: vstore::ai_content_hash_for_entry(conn, entry_id).unwrap(),
            provider_id: Some("test".into()),
            model: Some("test-model".into()),
            generated_at: Some("2026-07-17T00:00:00Z".into()),
            status: AiMetadataStatus::Ready,
        };
        vstore::set_ai_metadata(conn, &metadata).unwrap();
    }

    // ---- Plan 要求的两个 verbatim 测试 ----------------------------------

    #[test]
    fn chinese_substring_search_matches_unspaced_title() {
        let mut conn = open_test_db();
        vstore::create_entry(&mut conn, &input("生产数据库凭据")).unwrap();
        let hits = search_local(&conn, "数据库", None, 20).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn ai_alias_recalls_entry_older_than_one_hundred_rows() {
        let mut conn = open_test_db();
        let target = vstore::create_entry(&mut conn, &input("Old production DB")).unwrap();
        set_ready_metadata(&mut conn, &target.entry.id, &["prod-db"]);
        for n in 0..150 {
            vstore::create_entry(&mut conn, &input(&format!("new entry {n}"))).unwrap();
        }
        let plan = AiQueryPlan {
            aliases: vec!["prod-db".into()],
            ..Default::default()
        };
        let hits = search_local(&conn, "之前的生产库", Some(&plan), 20).unwrap();
        assert!(hits
            .iter()
            .any(|hit| hit.summary.entry.id == target.entry.id));
    }

    // ---- 额外覆盖测试 ----------------------------------------------------

    #[test]
    fn empty_query_returns_empty_result() {
        let mut conn = open_test_db();
        vstore::create_entry(&mut conn, &input("Foo")).unwrap();
        let hits = search_local(&conn, "   ", None, 20).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn limit_is_clamped_to_one_at_minimum() {
        let mut conn = open_test_db();
        vstore::create_entry(&mut conn, &input("Foo Bar")).unwrap();
        vstore::create_entry(&mut conn, &input("Foo Baz")).unwrap();
        let hits = search_local(&conn, "Foo", None, 0).unwrap();
        // clamp(1, 100) -> 至少 1 条
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn sensitive_field_value_is_not_searchable() {
        let mut conn = open_test_db();
        let detail = vstore::create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "Vault Cred".into(),
                fields: vec![FieldInput {
                    key: "password".into(),
                    value: "SENSITIVE_VALUE_XYZ".into(),
                    is_sensitive: true,
                }],
                notes: None,
                manual_tags: vec![],
            },
        )
        .unwrap();
        let hits = search_local(&conn, "SENSITIVE_VALUE_XYZ", None, 20).unwrap();
        assert!(
            !hits.iter().any(|h| h.summary.entry.id == detail.entry.id),
            "sensitive value must not be searchable"
        );
    }

    #[test]
    fn pending_metadata_does_not_participate_in_ai_expansion() {
        let mut conn = open_test_db();
        // 创建 entry 时 metadata 默认是 pending；alias 也不会落到 FTS。
        let detail = vstore::create_entry(&mut conn, &input("Some Title")).unwrap();
        // 故意直接 SQL 写入一条 ready+alias 但 content_hash 不匹配的 metadata
        // 来证明"hash 不一致 → 不参与 AI 扩展"
        conn.execute(
            "UPDATE vault_ai_metadata SET status='ready',
                search_aliases_json='[\"stale-alias\"]',
                content_hash='deadbeef'
             WHERE entry_id=?1",
            params![detail.entry.id],
        )
        .unwrap();
        // 不重建 FTS，确保 alias 没漏进 searchable；于是只能靠 metadata 关联
        let plan = AiQueryPlan {
            aliases: vec!["stale-alias".into()],
            ..Default::default()
        };
        let hits = search_local(&conn, "irrelevant query", Some(&plan), 20).unwrap();
        assert!(
            !hits.iter().any(|h| h.summary.entry.id == detail.entry.id),
            "stale metadata (hash mismatch) must not be recalled by AI alias"
        );
    }

    #[test]
    fn ready_metadata_participates_in_ai_alias_expansion() {
        let mut conn = open_test_db();
        let detail = vstore::create_entry(&mut conn, &input("Real Ready Entry")).unwrap();
        set_ready_metadata(&mut conn, &detail.entry.id, &["unique-alias-7"]);
        let plan = AiQueryPlan {
            aliases: vec!["unique-alias-7".into()],
            ..Default::default()
        };
        let hits = search_local(&conn, "完全无关的查询", Some(&plan), 20).unwrap();
        assert!(
            hits.iter().any(|h| h.summary.entry.id == detail.entry.id),
            "ready+fresh metadata must participate in AI alias recall"
        );
    }

    #[test]
    fn kind_filter_drops_non_matching_entries() {
        let mut conn = open_test_db();
        let cred = vstore::create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "Kind Test".into(),
                fields: vec![],
                notes: None,
                manual_tags: vec![],
            },
        )
        .unwrap();
        let bm = vstore::create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Bookmark,
                title: "Kind Test".into(),
                fields: vec![],
                notes: None,
                manual_tags: vec![],
            },
        )
        .unwrap();
        let plan = AiQueryPlan {
            kinds: vec![EntryKind::Bookmark],
            ..Default::default()
        };
        let hits = search_local(&conn, "Kind Test", Some(&plan), 20).unwrap();
        assert!(hits.iter().any(|h| h.summary.entry.id == bm.entry.id));
        assert!(!hits.iter().any(|h| h.summary.entry.id == cred.entry.id));
    }

    #[test]
    fn date_filter_drops_entries_outside_range() {
        let mut conn = open_test_db();
        let detail = vstore::create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Note,
                title: "Date Test".into(),
                fields: vec![],
                notes: None,
                manual_tags: vec![],
            },
        )
        .unwrap();
        // 强制把 created_at 改到 2020 年
        conn.execute(
            "UPDATE vault_entries SET created_at='2020-01-01T00:00:00+00:00' WHERE id=?1",
            params![detail.entry.id],
        )
        .unwrap();
        let plan = AiQueryPlan {
            date_from: Some("2026-01-01".into()),
            date_to: Some("2026-12-31".into()),
            ..Default::default()
        };
        let hits = search_local(&conn, "Date Test", Some(&plan), 20).unwrap();
        assert!(
            !hits.iter().any(|h| h.summary.entry.id == detail.entry.id),
            "entry created in 2020 must be filtered out by 2026 range"
        );
    }

    #[test]
    fn entry_id_dedup_single_card_per_entry() {
        let mut conn = open_test_db();
        // 一个 entry 的 title / notes / field / tag 都包含同一个 keyword
        let detail = vstore::create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "alpha".into(),
                fields: vec![FieldInput {
                    key: "user".into(),
                    value: "alpha".into(),
                    is_sensitive: false,
                }],
                notes: Some("alpha notes".into()),
                manual_tags: vec!["alpha".into()],
            },
        )
        .unwrap();
        let hits = search_local(&conn, "alpha", None, 20).unwrap();
        // 同一 entry 不应出现多次
        let count = hits
            .iter()
            .filter(|h| h.summary.entry.id == detail.entry.id)
            .count();
        assert_eq!(count, 1, "duplicate cards not allowed");
    }

    #[test]
    fn local_source_priority_higher_than_ai_expansion() {
        let mut conn = open_test_db();
        let detail = vstore::create_entry(&mut conn, &input("LocalPriority Test")).unwrap();
        set_ready_metadata(&mut conn, &detail.entry.id, &["localpriority-alias"]);
        // 同 entry 同时被原查询 title 命中（100）和 AI alias 命中（45）
        let plan = AiQueryPlan {
            aliases: vec!["localpriority-alias".into()],
            ..Default::default()
        };
        let hits = search_local(&conn, "LocalPriority", Some(&plan), 20).unwrap();
        let hit = hits
            .iter()
            .find(|h| h.summary.entry.id == detail.entry.id)
            .unwrap();
        // Local 一定在 sources 里
        assert!(hit.sources.contains(&SearchSource::Local));
        // 分数 >= 100（title 基础分，AI alias 作为额外 term 加 3）
        assert!(
            hit.score >= 100.0,
            "local title match should dominate; got score = {}",
            hit.score
        );
    }

    #[test]
    fn multiple_terms_bonus_capped_at_fifteen() {
        let mut conn = open_test_db();
        // 用 6 个独立 term 命中同一 entry：title (1 term) + 5 个 AI keywords
        let detail = vstore::create_entry(&mut conn, &input("MultiBonus Title")).unwrap();
        set_ready_metadata(&mut conn, &detail.entry.id, &[]);
        // 把 5 个独立 keyword 都注入 searchable（通过手动 tags）
        vstore::set_manual_tags(
            &mut conn,
            &detail.entry.id,
            &[
                "kb1".into(),
                "kb2".into(),
                "kb3".into(),
                "kb4".into(),
                "kb5".into(),
            ],
        )
        .unwrap();
        let plan = AiQueryPlan {
            keywords: vec![
                "kb1".into(),
                "kb2".into(),
                "kb3".into(),
                "kb4".into(),
                "kb5".into(),
            ],
            ..Default::default()
        };
        // 原查询只匹配 title（1 term）
        let hits = search_local(&conn, "MultiBonus", Some(&plan), 20).unwrap();
        let hit = hits
            .iter()
            .find(|h| h.summary.entry.id == detail.entry.id)
            .unwrap();
        // title(100) + 5 extra terms × 3 = 15 → 总分 115
        assert!(
            hit.score <= 100.0 + MAX_BONUS,
            "bonus must be capped at 15; got {}",
            hit.score
        );
        assert!(
            hit.score >= 100.0,
            "title base score must be retained; got {}",
            hit.score
        );
    }
}
