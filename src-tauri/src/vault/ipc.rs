// src-tauri/src/vault/ipc.rs
use std::sync::Mutex;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::storage::error::{StorageError, StorageResult};
use crate::vault::desensitize::{desensitize_entry, desensitize_raw_text, TokenMap};
use crate::vault::llm::openai_compat::OpenAiCompatAdapter;
use crate::vault::llm::presets::{find_preset, ProviderPreset, PRESETS};
use crate::vault::llm::prompt::{capture_enrichment_prompt, query_plan_prompt};
use crate::vault::llm::{LlmAdapter, LlmError, LlmRequest};
use crate::vault::models::{
    EntryKind, SearchSource, VaultEntryDetail, VaultEntryInput, VaultEntrySummary,
    VaultSearchHit,
};
use crate::vault::storage as vstore;

#[derive(Default)]
pub struct VaultRuntimeState {
    pub token_map: Mutex<TokenMap>,
    pub llm_config: Mutex<Option<LlmConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfig {
    pub provider_id: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TagUpdateEvent {
    pub id: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LlmErrorEvent {
    pub kind: String,
    pub message: String,
}

#[tauri::command]
pub async fn ipc_vault_create_entry(
    state: State<'_, crate::AppState>,
    app: AppHandle,
    input: VaultEntryInput,
) -> Result<VaultEntryDetail, String> {
    let detail = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        vstore::create_entry(&mut conn, &input).map_err(|e| e.to_string())?
    };

    // spawn 内通过 app.state::<T>() 重新获取——State<'_, T> 不是 'static 不能 move
    let app_for_spawn = app.clone();
    let entry_id = detail.entry.id.clone();
    tauri::async_runtime::spawn(async move {
        match suggest_tags_for_entry(entry_id.clone(), app_for_spawn.clone()).await {
            Ok(tags) => {
                let _ = app_for_spawn.emit(
                    "vault-tags-updated",
                    TagUpdateEvent { id: entry_id, tags },
                );
            }
            Err(_) => {}
        }
    });

    Ok(detail)
}

/// 异步打标——所有 state 通过 app.state() 重新获取
///
/// Task 7 之后改走结构化 capture enrichment：把脱敏后的 entry 文本交给
/// `capture_enrichment_prompt`，LLM 返回的 JSON 由 `parse_capture_response`
/// 严格校验（title/notes/fields detokenize_strict；tags/summary/aliases
/// 走 validate_non_sensitive_metadata）。本函数只取其中的 `ai_tags` 写回，
/// 但同样的 suggestion 后续可以被 Task 10 用来生成 aliases / summary。
async fn suggest_tags_for_entry(entry_id: String, app: AppHandle) -> Result<Vec<String>, ()> {
    // 1) 取 entry 详情（lock db → 取 → drop guard）
    let (entry, fields, tags) = {
        let app_state = app.state::<crate::AppState>();
        let conn = app_state.db.lock().map_err(|_| ())?;
        let detail = vstore::get_entry_detail(&conn, &entry_id).map_err(|_| ())?;
        // desensitize_entry 仍需要 Vec<String>；这里取 tag 的显示文本
        let tag_strings: Vec<String> = detail.tags.iter().map(|t| t.tag.clone()).collect();
        (detail.entry, detail.fields, tag_strings)
    };

    // 2) 取 LLM 配置（lock → clone → drop）
    let config: Option<LlmConfig> = {
        let vault_state = app.state::<VaultRuntimeState>();
        let guard = vault_state.llm_config.lock().unwrap();
        guard.clone()
    };
    let config = match config {
        Some(c) => c,
        None => return Err(()),
    };

    // 3) 脱敏（lock token_map → 处理 → drop，**不跨 await**）
    //    同时把脱敏后的 entry 展平成自由文本，交给 capture_enrichment_prompt。
    //    masked_text 是 LLM 实际会看到的全部用户数据 ——
    //    title / notes / fields 都经过 desensitize_entry 处理，
    //    绝不再把原始 entry 的 title 等敏感字段直接发给 LLM（I3 回归）。
    let masked_text = {
        let vault_state = app.state::<VaultRuntimeState>();
        let mut map = vault_state.token_map.lock().unwrap();
        let d_entry = desensitize_entry(&entry, &fields, &tags, &mut map);
        // 构造 LLM 看到的"用户数据"：title / notes / fields 拼成一段
        let mut buf = String::new();
        buf.push_str("title: ");
        buf.push_str(&d_entry.title);
        buf.push('\n');
        if !d_entry.notes.is_empty() {
            buf.push_str("notes: ");
            buf.push_str(&d_entry.notes);
            buf.push('\n');
        }
        for f in &d_entry.fields {
            buf.push_str(&format!("{}: {}\n", f.key, f.value));
        }
        if !d_entry.tags.is_empty() {
            buf.push_str(&format!("tags: {}\n", d_entry.tags.join(", ")));
        }
        buf
    };

    // 4) 调 LLM（不持任何 lock）
    let adapter = match OpenAiCompatAdapter::new(
        config.base_url, config.api_key, config.model,
    ) {
        Ok(a) => a,
        Err(_) => return Err(()),
    };
    let req = LlmRequest {
        messages: capture_enrichment_prompt(&masked_text),
        json_mode: true,
        temperature: 0.3,
        max_tokens: Some(512),
    };
    let resp = match adapter.complete(req).await {
        Ok(r) => r,
        Err(e) => {
            let _ = app.emit("vault-llm-error", llm_error_event(e));
            return Err(());
        }
    };

    // 5) 结构化解析 —— 在 token_map 的锁内执行 parse，因为
    //    parse_capture_response 需要按需 detokenize_strict 回填占位符。
    //    TokenMap 的内部字段是私有的，且 token 是请求级随机生成的，所以
    //    必须用与脱敏时同一个 map 实例，不能 clone 一份新的。
    //    解析是纯 CPU 操作，不会跨 await，可以安全持锁。
    let suggestion = {
        let vault_state = app.state::<VaultRuntimeState>();
        let map = vault_state.token_map.lock().unwrap();
        crate::vault::ai::parse_capture_response(&resp.content, &map)
    };
    let suggestion = match suggestion {
        Ok(s) => s,
        Err(e) => {
            let _ = app.emit("vault-llm-error", llm_error_event(e));
            return Err(());
        }
    };
    if suggestion.ai_tags.is_empty() {
        return Err(()); // 空标签视为不可用
    }
    {
        let app_state = app.state::<crate::AppState>();
        let mut conn = app_state.db.lock().map_err(|_| ())?;
        let _ = vstore::replace_ai_tags(&mut conn, &entry_id, &suggestion.ai_tags);
    }
    Ok(suggestion.ai_tags)
}

pub(crate) fn llm_error_event(e: LlmError) -> LlmErrorEvent {
    let (kind, msg) = match e {
        LlmError::Auth => ("auth", "API key 失效".to_string()),
        LlmError::RateLimit => ("rate_limit", "限流".to_string()),
        LlmError::Timeout => ("timeout", "超时".to_string()),
        LlmError::Network(m) => ("network", m),
        LlmError::Server(c, m) => ("server", format!("HTTP {c}: {m}")),
        LlmError::Parse(m) => ("parse", m),
        LlmError::InvalidConfig(m) => ("config", m),
    };
    LlmErrorEvent { kind: kind.into(), message: msg }
}

#[tauri::command]
pub async fn ipc_vault_update_entry(
    state: State<'_, crate::AppState>,
    id: String,
    input: VaultEntryInput,
) -> Result<VaultEntryDetail, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    vstore::update_entry(&mut conn, &id, &input).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ipc_vault_delete_entry(
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    vstore::delete_entry(&mut conn, &id).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn ipc_vault_list_entries(
    state: State<'_, crate::AppState>,
    kind: Option<EntryKind>,
) -> Result<Vec<VaultEntrySummary>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    vstore::list_entries(&conn, kind).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ipc_vault_get_entry(
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<VaultEntryDetail, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    vstore::get_entry_detail(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ipc_vault_update_tags(
    state: State<'_, crate::AppState>,
    id: String,
    tags: Vec<String>,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    vstore::set_manual_tags(&mut conn, &id, &tags).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn ipc_vault_retag(app: AppHandle, id: String) -> Result<(), String> {
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = suggest_tags_for_entry(id, app_clone).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn ipc_vault_search(
    state: State<'_, crate::AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<VaultSearchHit>, String> {
    // FTS5-only：用于 Vault header 的快速关键词搜索
    let limit = limit.unwrap_or(20);

    let fts_hits: Vec<(String, f64)> = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        vstore::fts5_search(&conn, &query, limit).map_err(|e| e.to_string())?
    };

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut hits = Vec::with_capacity(fts_hits.len());
    for (id, score) in fts_hits {
        if let Ok(Some(entry)) = vstore::get_entry_by_id(&conn, &id) {
            let tags = vstore::list_tags_with_source(&conn, &id).unwrap_or_default();
            let preview = entry.notes.clone();
            hits.push(VaultSearchHit {
                summary: VaultEntrySummary {
                    entry,
                    tags,
                    preview,
                },
                score,
                sources: vec![SearchSource::Local],
            });
        }
    }
    Ok(hits)
}

/// LLM 自然语言搜索（独立端点）：脱敏查询 → 调 LLM 生成结构化查询计划
/// → 用计划在本地做检索 → 返回匹配条目。
///
/// Task 7 之后这里不再把整份 catalog 喂给 LLM（之前通过 `search_prompt`
/// 把脱敏后的全部 entry 字段塞进 prompt，泄露面太大）。新流程：
///   1) 把查询本身脱敏，只把脱敏查询交给 `query_plan_prompt`；
///   2) LLM 返回结构化 AiQueryPlan，由 `parse_query_plan` 严格校验；
///   3) 计划无效 → 整次降级为本地 FTS5 搜索；
///   4) 计划有效 → 用 keywords 在 FTS5 上做联合检索。
#[tauri::command]
pub async fn ipc_vault_llm_search(
    state: State<'_, crate::AppState>,
    app: AppHandle,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<VaultSearchHit>, String> {
    let limit = limit.unwrap_or(20);

    // 1) 取 LLM 配置
    let config = {
        let vault_state = app.state::<VaultRuntimeState>();
        let guard = vault_state.llm_config.lock().unwrap();
        guard.clone()
    };
    let config = match config {
        Some(c) => c,
        None => return Ok(vec![]),
    };

    // 2) 脱敏查询（lock token_map → 处理 → drop）
    let masked_query = {
        let vault_state = app.state::<VaultRuntimeState>();
        let mut map = vault_state.token_map.lock().unwrap();
        desensitize_raw_text(&query, &[], &mut map)
    };
    let now_rfc3339 = chrono::Utc::now().to_rfc3339();

    // 3) 调 LLM 生成查询计划
    let adapter = match OpenAiCompatAdapter::new(config.base_url, config.api_key, config.model) {
        Ok(a) => a,
        Err(_) => return Ok(vec![]),
    };
    let req = LlmRequest {
        messages: query_plan_prompt(&masked_query, &now_rfc3339),
        json_mode: true,
        temperature: 0.0,
        max_tokens: Some(256),
    };
    let resp = match adapter.complete(req).await {
        Ok(r) => r,
        Err(e) => {
            let _ = app.emit("vault-llm-error", llm_error_event(e));
            // 网络错误降级为本地搜索
            return local_search(&state, &query, limit);
        }
    };

    // 4) 解析计划 —— 无效整次降级
    let plan = match crate::vault::ai::parse_query_plan(&resp.content) {
        Ok(p) => p,
        Err(e) => {
            let _ = app.emit("vault-llm-error", llm_error_event(e));
            return local_search(&state, &query, limit);
        }
    };

    // 5) 本地检索：优先用 plan.keywords 联合查询，没有就用原查询做 FTS5
    let effective_query = if plan.keywords.is_empty() {
        query.clone()
    } else {
        plan.keywords.join(" ")
    };
    local_search(&state, &effective_query, limit)
}

/// 本地 FTS5 搜索的统一封装。结果统一标记 `SearchSource::AiExpanded`，
/// 表示这是"经过 AI 查询理解后触发的检索"（即使 LLM 失败降级到这里也保持一致）。
fn local_search(
    state: &State<'_, crate::AppState>,
    query: &str,
    limit: usize,
) -> Result<Vec<VaultSearchHit>, String> {
    let fts_hits: Vec<(String, f64)> = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        vstore::fts5_search(&conn, query, limit).map_err(|e| e.to_string())?
    };
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut hits = Vec::with_capacity(fts_hits.len());
    for (id, score) in fts_hits {
        if let Ok(Some(entry)) = vstore::get_entry_by_id(&conn, &id) {
            let tags = vstore::list_tags_with_source(&conn, &id).unwrap_or_default();
            let preview = entry.notes.clone();
            hits.push(VaultSearchHit {
                summary: VaultEntrySummary {
                    entry,
                    tags,
                    preview,
                },
                score,
                sources: vec![SearchSource::AiExpanded],
            });
        }
    }
    Ok(hits)
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPresetDto {
    pub id: String,
    pub label: String,
    pub base_url: String,
    pub models: Vec<String>,
    pub default_model: String,
}

impl From<&ProviderPreset> for ProviderPresetDto {
    fn from(p: &ProviderPreset) -> Self {
        Self {
            id: p.id.into(),
            label: p.label.into(),
            base_url: p.base_url.into(),
            models: p.models.iter().map(|s| s.to_string()).collect(),
            default_model: p.default_model.into(),
        }
    }
}

const LLM_CONFIG_PREF_KEY: &str = "vault_llm_config";

fn load_llm_config(conn: &rusqlite::Connection) -> Option<LlmConfig> {
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM preferences WHERE key=?1",
            params![LLM_CONFIG_PREF_KEY],
            |r| r.get(0),
        )
        .ok();
    v.and_then(|s| serde_json::from_str(&s).ok())
}

fn save_llm_config(conn: &mut rusqlite::Connection, cfg: &LlmConfig) -> StorageResult<()> {
    let s = serde_json::to_string(cfg).map_err(|e| StorageError::Other(e.to_string()))?;
    conn.execute(
        "INSERT INTO preferences(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![LLM_CONFIG_PREF_KEY, s],
    )?;
    Ok(())
}

#[tauri::command]
pub fn ipc_vault_get_llm_presets() -> Vec<ProviderPresetDto> {
    PRESETS.iter().map(ProviderPresetDto::from).collect()
}

#[tauri::command]
pub async fn ipc_vault_get_llm_config(
    state: State<'_, crate::AppState>,
    vault_state: State<'_, VaultRuntimeState>,
) -> Result<Option<LlmConfig>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let cfg = load_llm_config(&conn);
    drop(conn);
    // 同步到 runtime state（lock → assign → drop）
    *vault_state.llm_config.lock().unwrap() = cfg.clone();
    Ok(cfg)
}

#[tauri::command]
pub async fn ipc_vault_set_llm_config(
    state: State<'_, crate::AppState>,
    vault_state: State<'_, VaultRuntimeState>,
    config: LlmConfig,
) -> Result<(), String> {
    // 应用 base_url 默认（如果 provider 是预设的且用户没改）
    let mut cfg = config;
    if let Some(p) = find_preset(&cfg.provider_id) {
        if cfg.base_url.is_empty() {
            cfg.base_url = p.base_url.into();
        }
    }
    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        save_llm_config(&mut conn, &cfg).map_err(|e| e.to_string())?;
    }
    *vault_state.llm_config.lock().unwrap() = Some(cfg);
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmTestResult {
    pub ok: bool,
    pub message: String,
    pub model_echo: Option<String>,
}

#[tauri::command]
pub async fn ipc_vault_test_llm(
    config: LlmConfig,
) -> Result<LlmTestResult, String> {
    let base_url = if config.base_url.is_empty() {
        find_preset(&config.provider_id).map(|p| p.base_url.to_string()).unwrap_or_default()
    } else {
        config.base_url
    };
    if base_url.is_empty() || config.api_key.is_empty() || config.model.is_empty() {
        return Ok(LlmTestResult { ok: false, message: "配置不完整".into(), model_echo: None });
    }

    let adapter = match OpenAiCompatAdapter::new(base_url, config.api_key.clone(), config.model.clone()) {
        Ok(a) => a,
        Err(e) => return Ok(LlmTestResult { ok: false, message: format!("{e:?}"), model_echo: None }),
    };

    let req = LlmRequest {
        messages: vec![crate::vault::llm::ChatMessage::user("ping")],
        json_mode: false,
        temperature: 0.0,
        max_tokens: Some(8),
    };

    match adapter.complete(req).await {
        Ok(resp) => Ok(LlmTestResult {
            ok: true,
            message: format!("响应 {} 字节", resp.content.len()),
            model_echo: Some(config.model),
        }),
        Err(e) => Ok(LlmTestResult {
            ok: false,
            message: format!("{e:?}"),
            model_echo: None,
        }),
    }
}
