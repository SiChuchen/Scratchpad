use std::collections::BTreeSet;

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

#[derive(Debug)]
struct SearchTerm {
    text: String,
    comparison_key: String,
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

pub fn search_local(
    conn: &Connection,
    query: &str,
    plan: Option<&UnifiedQueryPlan>,
    limit: usize,
) -> StorageResult<Vec<ContentSearchHit>> {
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

    let mut candidate_ids = BTreeSet::new();
    for term in &terms {
        candidate_ids.extend(matching_ids(conn, &term.text)?);
    }

    let mut hits = Vec::new();
    for unified_id in candidate_ids {
        let document = candidate_document(conn, &unified_id)?;
        if !passes_filters(document.kind, &document.created_at, plan) {
            continue;
        }
        let mut score = 0.0;
        let mut ai_contributed = false;
        for term in &terms {
            let term_score = score_term(&document, &term.comparison_key);
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

fn normalized_terms(query: &str, plan: Option<&UnifiedQueryPlan>) -> Vec<SearchTerm> {
    let original = normalize_text(query);
    let original_key = comparison_key(&original);
    let mut seen = BTreeSet::new();
    let mut terms = Vec::new();
    if !original.is_empty() {
        seen.insert(original_key.clone());
        terms.push(SearchTerm {
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

fn matching_ids(conn: &Connection, term: &str) -> StorageResult<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let expression = fts_expression(term);
    {
        let mut stmt = conn.prepare(
            "SELECT unified_id FROM content_fts
             WHERE content_fts MATCH ?1
             ORDER BY unified_id ASC",
        )?;
        let matched = stmt
            .query_map(params![expression], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.extend(matched);
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
        ids.extend(matched);
    }
    Ok(ids)
}

fn fts_expression(term: &str) -> String {
    let tokens: Vec<String> = term
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\""))
        .collect();
    if tokens.is_empty() {
        "\"contentsearchnomatchtoken\"".to_string()
    } else {
        tokens.join(" ")
    }
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

fn score_term(document: &CandidateDocument, term: &str) -> f64 {
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
    use crate::vault::storage::ensure_vault_schema;

    fn initialized_payload_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        ensure_dock_schema(&mut conn, 7).unwrap();
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
}
