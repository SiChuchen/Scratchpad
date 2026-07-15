// src-tauri/src/vault/ipc.rs
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::vault::desensitize::{desensitize_entry, TokenMap};
use crate::vault::llm::openai_compat::OpenAiCompatAdapter;
use crate::vault::llm::prompt::tag_prompt;
use crate::vault::llm::{LlmAdapter, LlmError, LlmRequest};
use crate::vault::models::{VaultEntry, VaultEntryInput};
use crate::vault::storage;

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
) -> Result<VaultEntry, String> {
    let entry = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        storage::create_entry(&mut conn, &input).map_err(|e| e.to_string())?
    };

    // spawn 内通过 app.state::<T>() 重新获取——State<'_, T> 不是 'static 不能 move
    let app_for_spawn = app.clone();
    let entry_id = entry.id.clone();
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

    Ok(entry)
}

/// 异步打标——所有 state 通过 app.state() 重新获取
async fn suggest_tags_for_entry(entry_id: String, app: AppHandle) -> Result<Vec<String>, ()> {
    // 1) 取 entry 详情（lock db → 取 → drop guard）
    let (entry, fields, tags) = {
        let app_state = app.state::<crate::AppState>();
        let conn = app_state.db.lock().map_err(|_| ())?;
        let detail = storage::get_entry_detail(&conn, &entry_id).map_err(|_| ())?;
        (detail.entry, detail.fields, detail.tags)
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
    let d_entry = {
        let vault_state = app.state::<VaultRuntimeState>();
        let mut map = vault_state.token_map.lock().unwrap();
        desensitize_entry(&entry, &fields, &tags, &mut map)
    };

    // 4) 调 LLM（不持任何 lock）
    let adapter = match OpenAiCompatAdapter::new(
        config.base_url, config.api_key, config.model,
    ) {
        Ok(a) => a,
        Err(_) => return Err(()),
    };
    let req = LlmRequest {
        messages: tag_prompt(&d_entry),
        json_mode: true,
        temperature: 0.3,
        max_tokens: Some(256),
    };
    let resp = match adapter.complete(req).await {
        Ok(r) => r,
        Err(e) => {
            let _ = app.emit("vault-llm-error", llm_error_event(e));
            return Err(());
        }
    };

    // 5) 解析 + 写回（lock db → 写 → drop）
    #[derive(serde::Deserialize)]
    struct TagResp { #[serde(default)] tags: Vec<String> }
    let parsed: TagResp = serde_json::from_str(&resp.content).unwrap_or(TagResp { tags: vec![] });
    if parsed.tags.is_empty() {
        return Err(()); // 空标签视为不可用
    }
    {
        let app_state = app.state::<crate::AppState>();
        let mut conn = app_state.db.lock().map_err(|_| ())?;
        let _ = storage::set_tags(&mut conn, &entry_id, &parsed.tags);
    }
    Ok(parsed.tags)
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
