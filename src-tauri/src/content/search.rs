use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection, OptionalExtension};

use crate::content::catalog::{parse_kind, summaries_for_scope, summary_by_id};
use crate::content::models::{
    BrowseScope, ContentKind, ContentSearchHit, SearchSource, UnifiedQueryPlan,
};
use crate::content::projection::normalize_text;
use crate::storage::error::{StorageError, StorageResult};

const EXACT_TITLE_WEIGHT: f64 = 120.0;
const TITLE_PREFIX_WEIGHT: f64 = 80.0;
const TAG_WEIGHT: f64 = 55.0;
const BODY_WEIGHT: f64 = 30.0;
const ALIAS_WEIGHT: f64 = 20.0;
const MAX_QUERY_CHARS: usize = 512;
const MAX_TOKENS_PER_TERM: usize = 32;
const MAX_PLAN_TERMS: usize = 64;

#[derive(Debug)]
struct SearchTerm {
    text: String,
    comparison_key: String,
    tokens: Vec<String>,
    ai_expanded: bool,
}

struct CandidateDocument {
    kind: ContentKind,
    created_at: String,
    title: String,
    body: String,
    tags: String,
    aliases: String,
}

#[derive(Debug, Clone, Copy, Default)]
struct FtsFieldMask(u8);

impl FtsFieldMask {
    const TITLE: u8 = 1 << 0;
    const BODY: u8 = 1 << 1;
    const TAGS: u8 = 1 << 2;
    const ALIASES: u8 = 1 << 3;

    fn insert(&mut self, field: u8) {
        self.0 |= field;
    }

    fn contains(self, field: u8) -> bool {
        self.0 & field != 0
    }
}

pub fn search_local(
    conn: &Connection,
    query: &str,
    plan: Option<&UnifiedQueryPlan>,
    limit: usize,
) -> StorageResult<Vec<ContentSearchHit>> {
    validate_search_input(query, plan)?;
    ensure_complete_projections(conn)?;
    let terms = normalized_terms(query, plan);
    let limit = clamp_limit(limit);
    if terms.is_empty() {
        return summaries_for_scope(conn, BrowseScope::All, false)?
            .into_iter()
            .filter(|summary| passes_filters(summary.kind, &summary.created_at, plan))
            .take(limit)
            .map(|summary| {
                Ok(ContentSearchHit {
                    summary,
                    score: 0.0,
                    sources: vec![SearchSource::Local],
                })
            })
            .collect();
    }

    let mut candidates: BTreeMap<String, Vec<FtsFieldMask>> = BTreeMap::new();
    for (term_index, term) in terms.iter().enumerate() {
        for (unified_id, fields) in matching_fields(conn, &term.text, &term.tokens)? {
            let term_fields = candidates
                .entry(unified_id)
                .or_insert_with(|| vec![FtsFieldMask::default(); terms.len()]);
            term_fields[term_index].0 |= fields.0;
        }
    }

    let mut hits = Vec::new();
    for (unified_id, term_fields) in candidates {
        let document = candidate_document(conn, &unified_id)?;
        if !passes_filters(document.kind, &document.created_at, plan) {
            continue;
        }
        let mut score = 0.0;
        let mut ai_contributed = false;
        for (term, fields) in terms.iter().zip(term_fields) {
            let term_score = score_term(&document, &term.comparison_key, &term.tokens, fields);
            if term_score > 0.0 {
                score += term_score;
                ai_contributed |= term.ai_expanded;
            }
        }
        if score == 0.0 {
            continue;
        }
        let mut sources = vec![SearchSource::Local];
        if ai_contributed {
            sources.push(SearchSource::AiExpanded);
        }
        hits.push(ContentSearchHit {
            summary: summary_by_id(conn, &unified_id, false)?,
            score,
            sources,
        });
    }
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| right.summary.updated_at.cmp(&left.summary.updated_at))
            .then_with(|| left.summary.id.cmp(&right.summary.id))
    });
    hits.truncate(limit);
    Ok(hits)
}

pub(crate) fn clamp_limit(limit: usize) -> usize {
    limit.clamp(1, 100)
}

fn validate_search_input(query: &str, plan: Option<&UnifiedQueryPlan>) -> StorageResult<()> {
    validate_term_boundary("query", query)?;
    if let Some(plan) = plan {
        let plan_term_count = plan
            .keywords
            .len()
            .checked_add(plan.aliases.len())
            .ok_or_else(|| StorageError::Validation("too many search plan terms".to_string()))?;
        if plan_term_count > MAX_PLAN_TERMS {
            return Err(StorageError::Validation(format!(
                "search plan terms exceed the limit of {MAX_PLAN_TERMS}"
            )));
        }
        for term in plan.keywords.iter().chain(&plan.aliases) {
            validate_term_boundary("search plan term", term)?;
        }
    }
    Ok(())
}

fn validate_term_boundary(label: &str, value: &str) -> StorageResult<()> {
    if value.chars().count() > MAX_QUERY_CHARS {
        return Err(StorageError::Validation(format!(
            "{label} exceeds the limit of {MAX_QUERY_CHARS} Unicode characters"
        )));
    }
    if unicode_tokens(value).len() > MAX_TOKENS_PER_TERM {
        return Err(StorageError::Validation(format!(
            "{label} exceeds the limit of {MAX_TOKENS_PER_TERM} tokens"
        )));
    }
    Ok(())
}

fn normalized_terms(query: &str, plan: Option<&UnifiedQueryPlan>) -> Vec<SearchTerm> {
    let original = normalize_text(query);
    let original_key = comparison_key(&original);
    let mut seen = BTreeSet::new();
    let mut terms = Vec::new();
    if !original.is_empty() {
        seen.insert(original_key.clone());
        terms.push(SearchTerm {
            tokens: unicode_tokens(&original),
            text: original,
            comparison_key: original_key.clone(),
            ai_expanded: false,
        });
    }
    if let Some(plan) = plan {
        for raw in plan.keywords.iter().chain(&plan.aliases) {
            let text = normalize_text(raw);
            if text.is_empty() {
                continue;
            }
            let key = comparison_key(&text);
            if seen.insert(key.clone()) {
                terms.push(SearchTerm {
                    tokens: unicode_tokens(&text),
                    text,
                    comparison_key: key.clone(),
                    ai_expanded: original_key != key,
                });
            }
        }
    }
    terms
}

fn ensure_complete_projections(conn: &Connection) -> StorageResult<()> {
    let incomplete = conn
        .query_row(
            "SELECT c.unified_id, COUNT(f.unified_id)
             FROM content_catalog c
             LEFT JOIN content_fts f ON f.unified_id = c.unified_id
             GROUP BY c.unified_id
             HAVING COUNT(f.unified_id) != 1
             ORDER BY c.unified_id ASC
             LIMIT 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    if let Some((unified_id, count)) = incomplete {
        return Err(StorageError::Validation(format!(
            "expected one safe projection for {unified_id}, found {count}"
        )));
    }
    Ok(())
}

fn matching_fields(
    conn: &Connection,
    term: &str,
    tokens: &[String],
) -> StorageResult<BTreeMap<String, FtsFieldMask>> {
    let mut matches = BTreeMap::new();
    let expression = fts_expression(tokens);
    {
        let mut stmt = conn.prepare(
            "SELECT unified_id, 1 AS field FROM content_fts
             WHERE content_fts MATCH ?1
             UNION ALL
             SELECT unified_id, 2 AS field FROM content_fts
             WHERE content_fts MATCH ?2
             UNION ALL
             SELECT unified_id, 3 AS field FROM content_fts
             WHERE content_fts MATCH ?3
             UNION ALL
             SELECT unified_id, 4 AS field FROM content_fts
             WHERE content_fts MATCH ?4
             ORDER BY unified_id ASC, field ASC",
        )?;
        let matched = stmt
            .query_map(
                params![
                    fts_field_expression("title", &expression),
                    fts_field_expression("body", &expression),
                    fts_field_expression("tags", &expression),
                    fts_field_expression("aliases", &expression),
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        for (unified_id, field) in matched {
            let field = match field {
                1 => FtsFieldMask::TITLE,
                2 => FtsFieldMask::BODY,
                3 => FtsFieldMask::TAGS,
                4 => FtsFieldMask::ALIASES,
                _ => {
                    return Err(StorageError::Validation(
                        "unknown FTS search field".to_string(),
                    ))
                }
            };
            matches
                .entry(unified_id)
                .or_insert_with(FtsFieldMask::default)
                .insert(field);
        }
    }
    {
        let mut stmt = conn.prepare(
            "SELECT unified_id FROM content_fts
             WHERE instr(lower(title), lower(?1)) > 0
                OR instr(lower(body), lower(?1)) > 0
                OR instr(lower(tags), lower(?1)) > 0
                OR instr(lower(aliases), lower(?1)) > 0
             ORDER BY unified_id ASC",
        )?;
        let matched = stmt
            .query_map(params![term], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for unified_id in matched {
            matches.entry(unified_id).or_default();
        }
    }
    Ok(matches)
}

fn unicode_tokens(term: &str) -> Vec<String> {
    term.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(comparison_key)
        .collect()
}

fn fts_expression(tokens: &[String]) -> String {
    if tokens.is_empty() {
        "\"contentsearchnomatchtoken\"".to_string()
    } else {
        tokens
            .iter()
            .map(|token| format!("\"{token}\""))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn fts_field_expression(field: &str, expression: &str) -> String {
    format!("{field} : ({expression})")
}

fn candidate_document(conn: &Connection, unified_id: &str) -> StorageResult<CandidateDocument> {
    let row = conn
        .query_row(
            "SELECT c.kind, c.created_at, f.title, f.body, f.tags, f.aliases
             FROM content_catalog c
             JOIN content_fts f ON f.unified_id = c.unified_id
             WHERE c.unified_id = ?1",
            params![unified_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Validation(format!("search candidate not found: {unified_id}"))
        })?;
    Ok(CandidateDocument {
        kind: parse_kind(&row.0, unified_id)?,
        created_at: row.1,
        title: row.2,
        body: row.3,
        tags: row.4,
        aliases: row.5,
    })
}

fn passes_filters(kind: ContentKind, created_at: &str, plan: Option<&UnifiedQueryPlan>) -> bool {
    let Some(plan) = plan else {
        return true;
    };
    if !plan.kinds.is_empty() && !plan.kinds.contains(&kind) {
        return false;
    }
    let created_date: String = created_at.chars().take(10).collect();
    if let Some(date_from) = plan.date_from.as_deref().map(normalize_text) {
        if !date_from.is_empty() && created_date < date_from {
            return false;
        }
    }
    if let Some(date_to) = plan.date_to.as_deref().map(normalize_text) {
        if !date_to.is_empty() && created_date > date_to {
            return false;
        }
    }
    true
}

fn score_term(
    document: &CandidateDocument,
    term: &str,
    tokens: &[String],
    fts_fields: FtsFieldMask,
) -> f64 {
    let title = comparison_key(&document.title);
    let body = comparison_key(&document.body);
    let tags = comparison_key(&document.tags);
    let aliases = comparison_key(&document.aliases);
    let mut score = 0.0_f64;
    if title == term {
        score = score.max(EXACT_TITLE_WEIGHT);
    }
    if title_prefix_matches(&title, term) {
        score = score.max(TITLE_PREFIX_WEIGHT);
    }
    if tags.contains(term) {
        score = score.max(TAG_WEIGHT);
    }
    if body.contains(term) {
        score = score.max(BODY_WEIGHT);
    }
    if aliases.contains(term) {
        score = score.max(ALIAS_WEIGHT);
    }
    if !tokens.is_empty() {
        if tokens
            .iter()
            .all(|token| title_prefix_matches(&title, token))
        {
            score = score.max(TITLE_PREFIX_WEIGHT);
        }
        if tokens.iter().all(|token| tags.contains(token)) {
            score = score.max(TAG_WEIGHT);
        }
        if tokens.iter().all(|token| body.contains(token)) {
            score = score.max(BODY_WEIGHT);
        }
        if tokens.iter().all(|token| aliases.contains(token)) {
            score = score.max(ALIAS_WEIGHT);
        }
    }
    if fts_fields.contains(FtsFieldMask::TITLE) {
        score = score.max(TITLE_PREFIX_WEIGHT);
    }
    if fts_fields.contains(FtsFieldMask::TAGS) {
        score = score.max(TAG_WEIGHT);
    }
    if fts_fields.contains(FtsFieldMask::BODY) {
        score = score.max(BODY_WEIGHT);
    }
    if fts_fields.contains(FtsFieldMask::ALIASES) {
        score = score.max(ALIAS_WEIGHT);
    }
    score
}

fn comparison_key(value: &str) -> String {
    value.to_lowercase()
}

fn title_prefix_matches(title: &str, term: &str) -> bool {
    title.match_indices(term).any(|(index, _)| {
        index == 0
            || title[..index]
                .chars()
                .next_back()
                .is_some_and(|character| !character.is_alphanumeric())
    })
}

#[cfg(test)]
mod tests {
    use rusqlite::{params, Connection};

    use super::{clamp_limit, search_local};
    use crate::content::migrations::ensure_content_schema;
    use crate::content::models::{ContentKind, SearchSource, UnifiedQueryPlan};
    use crate::content::projection::tests::{
        fixture_with_all_kinds, FILE_PATH_LITERAL, SENSITIVE_LITERAL,
    };
    use crate::scratchpad::storage::ensure_dock_schema;
    use crate::storage::error::StorageError;
    use crate::vault::storage::ensure_vault_schema;

    fn initialized_payload_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        ensure_dock_schema(&mut conn).unwrap();
        ensure_vault_schema(&mut conn).unwrap();
        conn
    }

    fn fixture_with_equal_score_rows() -> Connection {
        let mut conn = initialized_payload_db();
        for id in ["a", "b"] {
            conn.execute(
                "INSERT INTO entries(
                    id, kind, content, title, source, created_at, updated_at
                 ) VALUES (?1, 'text', 'open console access', ?2, 'fixture',
                           '2026-07-10T08:00:00+00:00',
                           '2026-07-11T08:00:00+00:00')",
                params![id, format!("Dock {id}")],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO vault_entries(id, kind, title, notes, created_at, updated_at)
             VALUES ('newer', 'note', 'Newer note', 'open console access',
                     '2026-07-10T08:00:00+00:00', '2026-07-12T08:00:00+00:00')",
            [],
        )
        .unwrap();
        ensure_content_schema(&mut conn, 7).unwrap();
        conn
    }

    fn fixture_with_ranked_rows() -> Connection {
        let mut conn = initialized_payload_db();
        let dock_rows = [
            ("rank-exact", "irrelevant payload", "console"),
            ("rank-prefix", "guide instructions", "console guide"),
            ("rank-body", "open console later", "Body item"),
            ("rank-middle", "database maintenance window", "Middle body"),
            ("rank-reverse", "window database", "Reverse body"),
            ("rank-hyphen", "foo bar", "Hyphen body"),
            ("rank-split", "window", "database"),
            (
                "rank-title-tokens",
                "irrelevant title payload",
                "database scheduled window",
            ),
            ("rank-percent", "Budget 100% ready", "Percent body"),
            (
                "rank-accented-title",
                "irrelevant accented title",
                "Café handbook",
            ),
            (
                "rank-plain-title",
                "irrelevant plain title",
                "Cafe handbook",
            ),
            ("rank-accented-body", "visit café tonight", "Accented body"),
            ("rank-plain-body", "visit cafe tonight", "Plain body"),
        ];
        for (id, content, title) in dock_rows {
            conn.execute(
                "INSERT INTO entries(
                    id, kind, content, title, source, created_at, updated_at
                 ) VALUES (?1, 'text', ?2, ?3, 'fixture',
                           '2026-07-10T08:00:00+00:00',
                           '2026-07-10T08:00:00+00:00')",
                params![id, content, title],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO vault_entries(id, kind, title, notes, created_at, updated_at)
             VALUES
             ('rank-tag', 'note', 'Tag item', 'tag payload',
              '2026-07-10T08:00:00+00:00', '2026-07-10T08:00:00+00:00'),
             ('rank-alias', 'note', 'Alias item', 'alias payload',
              '2026-07-10T08:00:00+00:00', '2026-07-10T08:00:00+00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vault_tags(entry_id, tag, normalized_tag, source)
             VALUES ('rank-tag', 'console', 'console', 'manual')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vault_ai_metadata(
                entry_id, summary, search_aliases_json, content_hash, status
             ) VALUES ('rank-alias', NULL, '[\"console\"]', 'rank-hash', 'ready')",
            [],
        )
        .unwrap();
        ensure_content_schema(&mut conn, 7).unwrap();
        conn
    }

    fn ids(hits: &[crate::content::models::ContentSearchHit]) -> Vec<&str> {
        hits.iter().map(|hit| hit.summary.id.as_str()).collect()
    }

    #[test]
    fn unified_search_finds_useful_temporary_and_saved_fields_without_private_text() {
        let conn = fixture_with_all_kinds();

        assert_eq!(
            ids(&search_local(&conn, "维护", None, 20).unwrap()),
            ["dock:text-1"]
        );
        assert_eq!(
            ids(&search_local(&conn, "生产", None, 20).unwrap()),
            ["vault:bookmark-1"]
        );
        assert_eq!(
            ids(&search_local(&conn, "alice", None, 20).unwrap()),
            ["vault:credential-1"]
        );
        assert_eq!(
            ids(&search_local(&conn, "上线", None, 20).unwrap()),
            ["dock:file-1"]
        );
        assert_eq!(
            ids(&search_local(&conn, "prod console", None, 20).unwrap()),
            ["vault:bookmark-1"]
        );
        let title_token = search_local(&conn, "console", None, 20).unwrap();
        assert_eq!(ids(&title_token), ["vault:bookmark-1"]);
        assert_eq!(title_token[0].score, 80.0);
        assert!(search_local(&conn, SENSITIVE_LITERAL, None, 20)
            .unwrap()
            .is_empty());
        assert!(search_local(&conn, FILE_PATH_LITERAL, None, 20)
            .unwrap()
            .is_empty());

        let serialized =
            serde_json::to_string(&search_local(&conn, "", None, 20).unwrap()).unwrap();
        assert!(!serialized.contains(SENSITIVE_LITERAL));
        assert!(!serialized.contains(FILE_PATH_LITERAL));
    }

    #[test]
    fn ranking_uses_each_terms_highest_field_weight_and_sums_distinct_terms() {
        let conn = fixture_with_ranked_rows();
        let hits = search_local(&conn, "console", None, 20).unwrap();

        assert_eq!(
            ids(&hits),
            [
                "dock:rank-exact",
                "dock:rank-prefix",
                "vault:rank-tag",
                "dock:rank-body",
                "vault:rank-alias",
            ]
        );
        assert_eq!(
            hits.iter().map(|hit| hit.score).collect::<Vec<_>>(),
            [120.0, 80.0, 55.0, 30.0, 20.0]
        );

        let plan = UnifiedQueryPlan {
            keywords: vec!["guide".into()],
            ..Default::default()
        };
        let summed = search_local(&conn, "console", Some(&plan), 20).unwrap();
        let prefix = summed
            .iter()
            .find(|hit| hit.summary.id == "dock:rank-prefix")
            .unwrap();
        assert_eq!(prefix.score, 160.0);
        assert_eq!(
            prefix.sources,
            [SearchSource::Local, SearchSource::AiExpanded]
        );
    }

    #[test]
    fn equal_scores_use_updated_at_then_stable_id() {
        let conn = fixture_with_equal_score_rows();
        let hits = search_local(&conn, "console", None, 20).unwrap();

        assert_eq!(ids(&hits), ["vault:newer", "dock:a", "dock:b"]);
        assert!(hits.windows(2).all(|pair| pair[0].score == pair[1].score));
    }

    #[test]
    fn multi_token_terms_score_once_per_field_without_requiring_phrase_adjacency_or_order() {
        let conn = fixture_with_ranked_rows();

        let spaced = search_local(&conn, "database window", None, 20).unwrap();
        assert_eq!(
            ids(&spaced),
            [
                "dock:rank-title-tokens",
                "dock:rank-middle",
                "dock:rank-reverse",
            ]
        );
        assert_eq!(
            spaced.iter().map(|hit| hit.score).collect::<Vec<_>>(),
            [80.0, 30.0, 30.0]
        );
        assert!(!ids(&spaced).contains(&"dock:rank-split"));

        let hyphenated = search_local(&conn, "foo-bar", None, 20).unwrap();
        assert_eq!(ids(&hyphenated), ["dock:rank-hyphen"]);
        assert_eq!(hyphenated[0].score, 30.0);

        let reversed = search_local(&conn, "bar foo", None, 20).unwrap();
        assert_eq!(ids(&reversed), ["dock:rank-hyphen"]);
        assert_eq!(reversed[0].score, 30.0);
    }

    #[test]
    fn punctuation_only_terms_use_literal_fallback_without_matching_every_row() {
        let conn = fixture_with_ranked_rows();

        let hits = search_local(&conn, "%", None, 20).unwrap();

        assert_eq!(ids(&hits), ["dock:rank-percent"]);
        assert_eq!(hits[0].score, 30.0);
    }

    #[test]
    fn unicode61_diacritic_matching_keeps_title_and_body_field_weights_in_both_directions() {
        let conn = fixture_with_ranked_rows();
        let expected = [
            ("dock:rank-accented-title", 80.0),
            ("dock:rank-plain-title", 80.0),
            ("dock:rank-accented-body", 30.0),
            ("dock:rank-plain-body", 30.0),
        ];

        for query in ["cafe", "café"] {
            let hits = search_local(&conn, query, None, 20).unwrap();
            let actual: Vec<(&str, f64)> = hits
                .iter()
                .map(|hit| (hit.summary.id.as_str(), hit.score))
                .collect();
            assert_eq!(actual, expected, "query {query:?}");
        }
    }

    #[test]
    fn empty_query_browses_in_stable_order_and_applies_kind_date_and_limit_filters() {
        let conn = fixture_with_all_kinds();
        let browse = search_local(&conn, " \u{0007} ", None, 20).unwrap();
        assert_eq!(
            ids(&browse),
            [
                "dock:text-1",
                "dock:image-1",
                "dock:file-1",
                "vault:credential-1",
                "vault:bookmark-1",
                "vault:note-1",
            ]
        );
        assert!(browse.iter().all(|hit| hit.score == 0.0));
        assert!(browse
            .iter()
            .all(|hit| hit.sources == [SearchSource::Local]));

        let plan = UnifiedQueryPlan {
            kinds: vec![ContentKind::File],
            keywords: vec![" ".into(), "\u{0000}".into()],
            date_from: Some("2026-07-12".into()),
            date_to: Some("2026-07-12".into()),
            ..Default::default()
        };
        assert_eq!(
            ids(&search_local(&conn, "", Some(&plan), 20).unwrap()),
            ["dock:file-1"]
        );
        assert_eq!(search_local(&conn, "", None, 0).unwrap().len(), 1);
        assert_eq!(clamp_limit(0), 1);
        assert_eq!(clamp_limit(1_000), 100);
    }

    #[test]
    fn plan_terms_are_stably_deduplicated_and_only_contributing_expansions_add_a_source() {
        let conn = fixture_with_all_kinds();
        let duplicate = UnifiedQueryPlan {
            keywords: vec![" ".into(), " alice ".into(), "ALICE".into()],
            aliases: vec!["\u{0000}".into(), "alice".into()],
            ..Default::default()
        };
        let local = search_local(&conn, "alice", Some(&duplicate), 20).unwrap();
        assert_eq!(ids(&local), ["vault:credential-1"]);
        assert_eq!(local[0].sources, [SearchSource::Local]);

        let plan_only = UnifiedQueryPlan {
            keywords: vec!["alice".into(), "alice".into(), " ".into()],
            ..Default::default()
        };
        let expanded = search_local(&conn, "", Some(&plan_only), 20).unwrap();
        assert_eq!(ids(&expanded), ["vault:credential-1"]);
        assert_eq!(
            expanded[0].sources,
            [SearchSource::Local, SearchSource::AiExpanded]
        );

        let alias_only = UnifiedQueryPlan {
            aliases: vec!["prod console".into()],
            kinds: vec![ContentKind::Bookmark],
            date_from: Some("2026-07-13".into()),
            date_to: Some("2026-07-13".into()),
            ..Default::default()
        };
        let alias_hit = search_local(&conn, "unmatched local", Some(&alias_only), 20).unwrap();
        assert_eq!(ids(&alias_hit), ["vault:bookmark-1"]);
        assert_eq!(
            alias_hit[0].sources,
            [SearchSource::Local, SearchSource::AiExpanded]
        );
    }

    #[test]
    fn fts_operators_quotes_and_like_wildcards_are_literal_and_never_raise_syntax_errors() {
        let conn = fixture_with_all_kinds();
        for query in ["\"", "foo\"bar", "OR", "-", "*", "%", "_"] {
            let hits = search_local(&conn, query, None, 20)
                .unwrap_or_else(|error| panic!("query {query:?} failed: {error}"));
            if matches!(query, "%" | "_") {
                assert!(hits.is_empty(), "query {query:?} acted like a wildcard");
            }
        }
    }

    #[test]
    fn oversized_queries_tokens_and_raw_plan_terms_fail_before_database_access() {
        let conn = Connection::open_in_memory().unwrap();
        let too_many_tokens = (0..33)
            .map(|index| format!("token{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let too_many_plan_terms = UnifiedQueryPlan {
            keywords: vec!["duplicate".to_string(); 65],
            ..Default::default()
        };
        let oversized_plan_term = UnifiedQueryPlan {
            aliases: vec!["界".repeat(513)],
            ..Default::default()
        };

        for result in [
            search_local(&conn, &"界".repeat(513), None, 20),
            search_local(&conn, &("a".repeat(512) + "\u{0000}"), None, 20),
            search_local(&conn, &too_many_tokens, None, 20),
            search_local(&conn, "", Some(&too_many_plan_terms), 20),
            search_local(&conn, "", Some(&oversized_plan_term), 20),
        ] {
            assert!(matches!(result.unwrap_err(), StorageError::Validation(_)));
        }
    }

    #[test]
    fn query_plan_and_token_limits_accept_their_unicode_character_boundaries() {
        let conn = fixture_with_all_kinds();
        let thirty_two_tokens = (0..32)
            .map(|index| format!("token{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let boundary_plan = UnifiedQueryPlan {
            keywords: vec![String::new(); 63],
            aliases: vec!["界".repeat(512)],
            ..Default::default()
        };

        assert!(search_local(&conn, &"界".repeat(512), None, 20).is_ok());
        assert!(search_local(&conn, &thirty_two_tokens, None, 20).is_ok());
        assert!(search_local(&conn, "", Some(&boundary_plan), 20).is_ok());
    }
}
