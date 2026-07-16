// src-tauri/src/vault/ipc/mod.rs
//
// Task 8: Vault IPC runtime state + entry/search commands.
//
// 重构后 `VaultRuntimeState` 不再持有 `TokenMap`（脱敏在每次请求内部用局部
// `TokenMap` 完成，便于请求结束即销毁）。新职责：
//   - 缓存启动时从 DB 加载的 LLM 配置和 AI 设置；
//   - 维护 auth-blocked / 连续网络失败计数 / cooldown 截止时间，自动门控
//     后台 AI 调用（前台手动测试不受门控影响）；
//   - 持有一个活跃搜索 `CancellationToken`（供 Task 9 使用）。
//
// 所有锁都是 `std::sync::Mutex`，因为持锁段不会跨 await。用户事件只
// 暴露稳定 code（auth / rateLimit / timeout / network / server / parse），
// 绝不把 `LlmError::Server` 的响应 body 或 reqwest 错误直接送到前端。

pub mod settings;
pub mod search;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

use crate::vault::config::{
    self, load_ai_settings, load_stored_config, LlmConfigStored, VaultAiSettings,
};
use crate::vault::desensitize::{desensitize_entry, TokenMap};
use crate::vault::llm::openai_compat::OpenAiCompatAdapter;
use crate::vault::llm::prompt::capture_enrichment_prompt;
use crate::vault::llm::{LlmAdapter, LlmError, LlmRequest};
use crate::vault::models::{
    EntryKind, VaultEntryDetail, VaultEntryInput, VaultEntrySummary, VaultSearchHit,
};
use crate::vault::storage as vstore;

// ---- 失败门控常量 -----------------------------------------------------------

/// 连续多少次 Network/Timeout 失败后进入 cooldown。
const NETWORK_FAILURE_THRESHOLD: u32 = 3;
/// Network/Timeout cooldown 时长。
const NETWORK_COOLDOWN_SECS: u64 = 30;
/// RateLimit cooldown 时长。
const RATE_LIMIT_COOLDOWN_SECS: u64 = 60;

// ---- 运行时状态 -------------------------------------------------------------

/// Vault AI 运行时共享状态。线程安全；持锁段不允许跨 await。
pub struct VaultRuntimeState {
    /// 最近一次已验证保存的 LLM 配置；启动时从 DB 加载，运行时与 DB 同步。
    pub config: Mutex<Option<LlmConfigStored>>,
    /// 用户可调的 AI 行为开关；启动时从 DB 加载。
    pub settings: Mutex<VaultAiSettings>,
    /// Auth 失败后置 true，阻断所有自动 AI 调用；仅在 verify 成功或删除
    /// 配置时清零。
    auth_blocked: Mutex<bool>,
    /// 连续 Network/Timeout 失败次数；任一成功请求归零。
    consecutive_network_failures: Mutex<u32>,
    /// Cooldown 截止时间（`std::time::Instant`）；过期后自动恢复。
    cooldown_until: Mutex<Option<Instant>>,
    /// 活跃搜索的取消 token：(request_id, token)。Task 9 会用到。
    active_search: Mutex<Option<(String, CancellationToken)>>,
}

impl VaultRuntimeState {
    /// 从 DB 加载配置 + 设置；不读取也不会修改 cooldown/auth 状态。
    /// 失败/缺失时回退到默认值，保证 runtime 始终可用。
    pub fn load(conn: &Connection) -> Self {
        let config = load_stored_config(conn);
        let settings = load_ai_settings(conn);
        Self {
            config: Mutex::new(config),
            settings: Mutex::new(settings),
            auth_blocked: Mutex::new(false),
            consecutive_network_failures: Mutex::new(0),
            cooldown_until: Mutex::new(None),
            active_search: Mutex::new(None),
        }
    }

    /// 配置概览（不含 API Key）。无配置时返回 `None`。
    pub fn config_summary(&self) -> Option<config::LlmConfigSummary> {
        let guard = self.config.lock().unwrap();
        guard.as_ref().map(|c| c.summary())
    }

    /// 当前 AI 设置（clone 出一份）。
    pub fn settings(&self) -> VaultAiSettings {
        self.settings.lock().unwrap().clone()
    }

    /// 写入 AI 设置（仅 runtime；持久化由 settings IPC 命令负责）。
    pub fn set_settings(&self, settings: VaultAiSettings) {
        *self.settings.lock().unwrap() = settings;
    }

    /// 测试通过后调用：原子地写入 DB + 更新 runtime + 清除 auth/cooldown。
    pub fn save_config(
        &self,
        conn: &mut Connection,
        stored: LlmConfigStored,
    ) -> Result<(), String> {
        config::save_stored_config(conn, &stored).map_err(|e| e.to_string())?;
        *self.config.lock().unwrap() = Some(stored);
        // 成功验证 → 解除 auth 阻断 + 清零失败计数 + 清除 cooldown
        *self.auth_blocked.lock().unwrap() = false;
        *self.consecutive_network_failures.lock().unwrap() = 0;
        *self.cooldown_until.lock().unwrap() = None;
        Ok(())
    }

    /// 删除配置：同时清 DB、runtime 配置、auth/cooldown、取消 active search。
    pub fn delete_config(&self, conn: &mut Connection) -> Result<(), String> {
        config::delete_stored_config(conn).map_err(|e| e.to_string())?;
        *self.config.lock().unwrap() = None;
        *self.auth_blocked.lock().unwrap() = false;
        *self.consecutive_network_failures.lock().unwrap() = 0;
        *self.cooldown_until.lock().unwrap() = None;
        // 取消正在进行的搜索
        if let Some((_, token)) = self.active_search.lock().unwrap().take() {
            token.cancel();
        }
        Ok(())
    }

    /// 显式设置/清除 auth-blocked 标志。
    pub fn set_auth_blocked(&self, blocked: bool) {
        *self.auth_blocked.lock().unwrap() = blocked;
    }

    /// 成功请求：失败计数归零。不解除 auth 阻断（只能靠 verify/delete）。
    pub fn record_success(&self) {
        *self.consecutive_network_failures.lock().unwrap() = 0;
    }

    /// 按失败类型更新 runtime 状态：
    /// - Auth → `auth_blocked = true`（持久阻断，直到 verify/delete）
    /// - RateLimit → 60 秒 cooldown
    /// - Network/Timeout → `consecutive_network_failures += 1`；达到 3 → 30 秒 cooldown
    /// - Parse → 只影响本次请求，不进入网络 cooldown
    /// - Server → 不参与门控（临时服务端错误，让用户自己重试）
    pub fn record_failure(&self, error: &LlmError) {
        match error {
            LlmError::Auth => {
                *self.auth_blocked.lock().unwrap() = true;
            }
            LlmError::RateLimit => {
                let mut guard = self.cooldown_until.lock().unwrap();
                *guard = Some(Instant::now() + Duration::from_secs(RATE_LIMIT_COOLDOWN_SECS));
            }
            LlmError::Network(_) | LlmError::Timeout => {
                let mut counter = self.consecutive_network_failures.lock().unwrap();
                *counter += 1;
                if *counter >= NETWORK_FAILURE_THRESHOLD {
                    let mut guard = self.cooldown_until.lock().unwrap();
                    *guard = Some(Instant::now() + Duration::from_secs(NETWORK_COOLDOWN_SECS));
                }
            }
            LlmError::Server(_, _) | LlmError::Parse(_) | LlmError::InvalidConfig(_) => {
                // 不参与门控
            }
            LlmError::Cancelled => {
                // 用户取消不计入失败门控
            }
        }
    }

    /// 自动调用前查询：若当前被 auth-blocked 或处于 cooldown，返回稳定
    /// code 字符串；否则返回 `None` 表示可以发起请求。
    pub fn should_skip_automatic_call(&self) -> Option<String> {
        if *self.auth_blocked.lock().unwrap() {
            return Some("auth".to_string());
        }
        let guard = self.cooldown_until.lock().unwrap();
        if let Some(until) = *guard {
            if Instant::now() < until {
                return Some("cooldown".to_string());
            }
        }
        None
    }

    /// 把 `LlmError` 映射到稳定的用户事件 code（绝不泄露响应 body）。
    /// 返回字符串来自闭集：auth / rateLimit / timeout / network / server /
    /// parse / config / cancelled。
    pub fn user_error_code(error: &LlmError) -> &'static str {
        match error {
            LlmError::Auth => "auth",
            LlmError::RateLimit => "rateLimit",
            LlmError::Timeout => "timeout",
            LlmError::Network(_) => "network",
            LlmError::Server(_, _) => "server",
            LlmError::Parse(_) => "parse",
            LlmError::InvalidConfig(_) => "config",
            LlmError::Cancelled => "cancelled",
        }
    }

    /// 测试当前是否处于 cooldown（仅用于测试断言；rateLimit/network 触发后都为 true）。
    #[cfg(test)]
    pub(crate) fn has_cooldown(&self) -> bool {
        let guard = self.cooldown_until.lock().unwrap();
        guard
            .map(|until| Instant::now() < until)
            .unwrap_or(false)
    }

    /// 测试当前 auth 是否被阻断（仅用于测试断言）。
    #[cfg(test)]
    pub(crate) fn is_auth_blocked(&self) -> bool {
        *self.auth_blocked.lock().unwrap()
    }

    // ---- 活跃搜索 token（Task 9 使用） ------------------------------------

    /// 注册一个活跃搜索的取消 token。如果已有活跃搜索，旧 token **不会**
    /// 被取消（调用方决定是否要替换；Task 9 通常先取旧 token 再注册新的）。
    /// 返回旧的 (request_id, token)（如果有）。
    pub fn set_active_search(
        &self,
        request_id: String,
        token: CancellationToken,
    ) -> Option<(String, CancellationToken)> {
        self.active_search.lock().unwrap().replace((request_id, token))
    }

    /// 取出当前活跃搜索的 token clone（不消费 slot）。
    pub fn active_search_token(&self) -> Option<CancellationToken> {
        self.active_search
            .lock()
            .unwrap()
            .as_ref()
            .map(|(_, t)| t.clone())
    }

    /// 判断当前活跃搜索的 request_id 是否等于传入值。
    /// Task 9 用它来决定是否真的取消（防止迟到 cleanup 误取消新查询）。
    pub fn active_search_id_matches(&self, request_id: &str) -> bool {
        self.active_search
            .lock()
            .unwrap()
            .as_ref()
            .map(|(id, _)| id == request_id)
            .unwrap_or(false)
    }

    /// 用 request_id 清除匹配的活跃搜索；不匹配则不动。
    pub fn clear_active_search(&self, request_id: &str) {
        let mut guard = self.active_search.lock().unwrap();
        if let Some((id, _)) = guard.as_ref() {
            if id == request_id {
                *guard = None;
            }
        }
    }
}

impl Default for VaultRuntimeState {
    fn default() -> Self {
        Self {
            config: Mutex::new(None),
            settings: Mutex::new(VaultAiSettings::default()),
            auth_blocked: Mutex::new(false),
            consecutive_network_failures: Mutex::new(0),
            cooldown_until: Mutex::new(None),
            active_search: Mutex::new(None),
        }
    }
}

// ---- 用户事件类型 -----------------------------------------------------------

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
    pub code: String,
}

/// 把 `LlmError` 转成前端可消费的稳定事件（不含响应 body / reqwest 详情）。
pub(crate) fn llm_error_event(e: LlmError) -> LlmErrorEvent {
    let code = VaultRuntimeState::user_error_code(&e);
    let kind = match &e {
        LlmError::Auth => "auth",
        LlmError::RateLimit => "rateLimit",
        LlmError::Timeout => "timeout",
        LlmError::Network(_) => "network",
        LlmError::Server(_, _) => "server",
        LlmError::Parse(_) => "parse",
        LlmError::InvalidConfig(_) => "config",
        LlmError::Cancelled => "cancelled",
    };
    LlmErrorEvent {
        kind: kind.to_string(),
        code: code.to_string(),
    }
}

// ---- Entry / search commands -----------------------------------------------

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
        if let Ok(tags) = suggest_tags_for_entry(entry_id.clone(), app_for_spawn.clone()).await {
            let _ = app_for_spawn.emit(
                "vault-tags-updated",
                TagUpdateEvent { id: entry_id, tags },
            );
        }
    });

    Ok(detail)
}

/// 异步打标——所有 state 通过 app.state() 重新获取。
///
/// Task 7 之后改走结构化 capture enrichment；Task 8 之后 `TokenMap`
/// 改成请求局部变量，不再常驻 `VaultRuntimeState`。门控：自动调用前查询
/// `should_skip_automatic_call`，被阻断时直接放弃。
async fn suggest_tags_for_entry(entry_id: String, app: AppHandle) -> Result<Vec<String>, ()> {
    let vault_state = app.state::<VaultRuntimeState>();

    // 门控：被 auth-blocked 或处于 cooldown → 直接放弃自动调用
    if vault_state.should_skip_automatic_call().is_some() {
        return Err(());
    }

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
    let config: Option<LlmConfigStored> = {
        let guard = vault_state.config.lock().unwrap();
        guard.clone()
    };
    let config = match config {
        Some(c) => c,
        None => return Err(()),
    };

    // 3) 脱敏：本请求专属 `TokenMap`，不跨请求、不跨 IPC 命令。
    //    masked_text 是 LLM 实际会看到的全部用户数据 ——
    //    title / notes / fields 都经过 desensitize_entry 处理。
    let mut token_map = TokenMap::new();
    let masked_text = {
        let d_entry =
            desensitize_entry(&entry, &fields, &tags, &mut token_map);
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
        Ok(r) => {
            vault_state.record_success();
            r
        }
        Err(e) => {
            // 失败门控：根据错误类型更新 runtime
            vault_state.record_failure(&e);
            let _ = app.emit("vault-llm-error", llm_error_event(e));
            return Err(());
        }
    };

    // 5) 结构化解析：token_map 是请求局部变量，跨步骤共享同一实例
    let suggestion = crate::vault::ai::parse_capture_response(&resp.content, &token_map);
    let suggestion = match suggestion {
        Ok(s) => s,
        Err(e) => {
            // parse 错误不影响门控，但仍通知前端
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
    // FTS5-only：用于 Vault header 的快速关键词搜索。
    // Task 9 之后完整的混合检索改由 `vault::ipc::search::ipc_vault_search_hybrid_local`
    // 提供；本命令保留作为前端轻量调用入口，行为与 hybrid local 的
    // "原查询 + 无 plan" 路径等价（但只返回 Local 来源，不涉及 AI 扩展）。
    let limit = limit.unwrap_or(20);

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let hits = crate::vault::search::search_local(&conn, &query, None, limit)
        .map_err(|e| e.to_string())?;
    Ok(hits)
}

// ---- Presets ---------------------------------------------------------------

use crate::vault::llm::presets::{ProviderPreset, PRESETS};

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

#[tauri::command]
pub fn ipc_vault_get_llm_presets() -> Vec<ProviderPresetDto> {
    PRESETS.iter().map(ProviderPresetDto::from).collect()
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    use crate::scratchpad::storage::ensure_dock_schema;
    use crate::vault::config::{save_ai_settings, save_stored_config};

    fn open_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn, 0).unwrap();
        conn
    }

    fn sample_stored() -> LlmConfigStored {
        LlmConfigStored {
            provider_id: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: "sk-secret".into(),
            model: "deepseek-chat".into(),
        }
    }

    // ---- load ---------------------------------------------------------------

    #[test]
    fn load_runtime_reads_saved_config_without_get_ipc() {
        let mut conn = open_db();
        save_stored_config(&mut conn, &sample_stored()).unwrap();
        // 直接重新 load runtime，不调用任何 IPC 命令
        let runtime = VaultRuntimeState::load(&conn);
        let cfg = runtime.config.lock().unwrap().clone();
        assert!(cfg.is_some(), "config should be loaded from DB");
        assert_eq!(cfg.unwrap().provider_id, "deepseek");
    }

    #[test]
    fn load_runtime_reads_saved_settings() {
        let mut conn = open_db();
        save_ai_settings(
            &mut conn,
            &VaultAiSettings {
                auto_enrich: false,
                auto_hybrid_search: true,
                sensitive_clipboard_clear_seconds: Some(60),
            },
        )
        .unwrap();
        let runtime = VaultRuntimeState::load(&conn);
        let s = runtime.settings();
        assert!(!s.auto_enrich);
        assert_eq!(s.sensitive_clipboard_clear_seconds, Some(60));
    }

    #[test]
    fn config_summary_never_returns_api_key() {
        let mut conn = open_db();
        save_stored_config(&mut conn, &sample_stored()).unwrap();
        let runtime = VaultRuntimeState::load(&conn);
        let summary = runtime.config_summary().unwrap();
        let json = serde_json::to_string(&summary).unwrap();
        assert!(!json.contains("sk-secret"));
        assert!(summary.has_api_key);
    }

    #[test]
    fn first_verified_config_enables_both_ai_features() {
        // 新 runtime（无任何已存设置）→ 默认 settings 应启用两个 AI 功能
        let conn = open_db();
        let runtime = VaultRuntimeState::load(&conn);
        let s = runtime.settings();
        assert!(s.auto_enrich);
        assert!(s.auto_hybrid_search);
        assert_eq!(s.sensitive_clipboard_clear_seconds, Some(30));
    }

    #[test]
    fn reverify_preserves_existing_feature_toggles() {
        let mut conn = open_db();
        save_stored_config(&mut conn, &sample_stored()).unwrap();
        save_ai_settings(
            &mut conn,
            &VaultAiSettings {
                auto_enrich: false,
                auto_hybrid_search: false,
                sensitive_clipboard_clear_seconds: None,
            },
        )
        .unwrap();
        let runtime = VaultRuntimeState::load(&conn);

        // 模拟 reverify：调用 save_config（verify 成功后写回）
        runtime.save_config(&mut conn, sample_stored()).unwrap();

        // settings 应保持原样
        let s = runtime.settings();
        assert!(!s.auto_enrich);
        assert!(!s.auto_hybrid_search);
        assert_eq!(s.sensitive_clipboard_clear_seconds, None);
    }

    #[test]
    fn delete_config_clears_database_and_runtime() {
        let mut conn = open_db();
        save_stored_config(&mut conn, &sample_stored()).unwrap();
        save_ai_settings(
            &mut conn,
            &VaultAiSettings {
                auto_enrich: false,
                auto_hybrid_search: true,
                sensitive_clipboard_clear_seconds: Some(30),
            },
        )
        .unwrap();
        let runtime = VaultRuntimeState::load(&conn);

        // 模拟 auth-blocked + cooldown，验证删除时也被清零
        runtime.set_auth_blocked(true);
        runtime.record_failure(&LlmError::RateLimit);

        runtime.delete_config(&mut conn).unwrap();

        // DB 行已删
        let runtime2 = VaultRuntimeState::load(&conn);
        assert!(runtime2.config.lock().unwrap().is_none());
        // runtime 内部状态全清
        assert!(!runtime.is_auth_blocked());
        assert!(!runtime.has_cooldown());
        assert!(runtime.should_skip_automatic_call().is_none());
        // settings 不被 delete_config 清（保留用户偏好）—— 但删除配置后
        // 自动功能事实上无法触发（无 config）；这里验证 settings 仍在 DB
        let s = runtime2.settings();
        assert!(!s.auto_enrich); // 用户上次保存的值
    }

    // ---- 门控 ----------------------------------------------------------------

    #[test]
    fn auth_failure_blocks_automatic_calls_until_config_changes() {
        let mut conn = open_db();
        save_stored_config(&mut conn, &sample_stored()).unwrap();
        let runtime = VaultRuntimeState::load(&conn);

        runtime.record_failure(&LlmError::Auth);
        assert!(runtime.is_auth_blocked());
        // 自动调用被阻断
        assert_eq!(
            runtime.should_skip_automatic_call(),
            Some("auth".to_string())
        );

        // reverify 成功 → 解除阻断
        runtime.save_config(&mut conn, sample_stored()).unwrap();
        assert!(!runtime.is_auth_blocked());
        assert!(runtime.should_skip_automatic_call().is_none());

        // 同样：删除配置也解除阻断
        runtime.record_failure(&LlmError::Auth);
        assert!(runtime.is_auth_blocked());
        runtime.delete_config(&mut conn).unwrap();
        assert!(!runtime.is_auth_blocked());
    }

    #[test]
    fn three_network_failures_start_thirty_second_cooldown() {
        let conn = open_db();
        let runtime = VaultRuntimeState::load(&conn);

        // 两次失败 → 尚未 cooldown
        runtime.record_failure(&LlmError::Network("e1".into()));
        runtime.record_failure(&LlmError::Network("e2".into()));
        assert!(!runtime.has_cooldown());

        // 第三次 → cooldown 激活
        runtime.record_failure(&LlmError::Network("e3".into()));
        assert!(runtime.has_cooldown());
        let skip = runtime.should_skip_automatic_call();
        assert_eq!(skip, Some("cooldown".to_string()));
    }

    #[test]
    fn timeout_also_counts_toward_network_cooldown() {
        let conn = open_db();
        let runtime = VaultRuntimeState::load(&conn);
        runtime.record_failure(&LlmError::Timeout);
        runtime.record_failure(&LlmError::Network("e".into()));
        runtime.record_failure(&LlmError::Timeout);
        assert!(runtime.has_cooldown());
    }

    #[test]
    fn rate_limit_triggers_sixty_second_cooldown() {
        let conn = open_db();
        let runtime = VaultRuntimeState::load(&conn);
        runtime.record_failure(&LlmError::RateLimit);
        assert!(runtime.has_cooldown());
        assert_eq!(
            runtime.should_skip_automatic_call(),
            Some("cooldown".to_string())
        );
    }

    #[test]
    fn success_resets_network_failures() {
        let conn = open_db();
        let runtime = VaultRuntimeState::load(&conn);
        runtime.record_failure(&LlmError::Network("e1".into()));
        runtime.record_failure(&LlmError::Network("e2".into()));
        runtime.record_success();
        // 再失败两次也不会进入 cooldown
        runtime.record_failure(&LlmError::Network("e3".into()));
        runtime.record_failure(&LlmError::Network("e4".into()));
        assert!(!runtime.has_cooldown());
    }

    #[test]
    fn parse_error_does_not_trigger_cooldown() {
        let conn = open_db();
        let runtime = VaultRuntimeState::load(&conn);
        for _ in 0..10 {
            runtime.record_failure(&LlmError::Parse("bad json".into()));
        }
        assert!(!runtime.has_cooldown());
        assert!(runtime.should_skip_automatic_call().is_none());
    }

    /// Task 9: Cancelled 是用户主动取消，绝不应触发 cooldown 或 auth-blocked。
    #[test]
    fn cancelled_does_not_trigger_cooldown_or_auth_block() {
        let conn = open_db();
        let runtime = VaultRuntimeState::load(&conn);
        runtime.record_failure(&LlmError::Cancelled);
        runtime.record_failure(&LlmError::Cancelled);
        assert!(!runtime.has_cooldown());
        assert!(!runtime.is_auth_blocked());
        assert!(runtime.should_skip_automatic_call().is_none());
    }

    // ---- user_error_code ----------------------------------------------------

    #[test]
    fn user_error_event_does_not_expose_provider_response_body() {
        let e = LlmError::Server(500, "INTERNAL_SECRET".to_string());
        let code = VaultRuntimeState::user_error_code(&e);
        assert_eq!(code, "server");
        let event = llm_error_event(LlmError::Server(500, "INTERNAL_SECRET".into()));
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("INTERNAL_SECRET"));
        assert_eq!(event.code, "server");
        assert_eq!(event.kind, "server");
    }

    #[test]
    fn user_error_code_covers_all_stable_kinds() {
        assert_eq!(VaultRuntimeState::user_error_code(&LlmError::Auth), "auth");
        assert_eq!(
            VaultRuntimeState::user_error_code(&LlmError::RateLimit),
            "rateLimit"
        );
        assert_eq!(
            VaultRuntimeState::user_error_code(&LlmError::Timeout),
            "timeout"
        );
        assert_eq!(
            VaultRuntimeState::user_error_code(&LlmError::Network("x".into())),
            "network"
        );
        assert_eq!(
            VaultRuntimeState::user_error_code(&LlmError::Server(502, "x".into())),
            "server"
        );
        assert_eq!(
            VaultRuntimeState::user_error_code(&LlmError::Parse("x".into())),
            "parse"
        );
        assert_eq!(
            VaultRuntimeState::user_error_code(&LlmError::InvalidConfig("x".into())),
            "config"
        );
        assert_eq!(
            VaultRuntimeState::user_error_code(&LlmError::Cancelled),
            "cancelled"
        );
    }
}
