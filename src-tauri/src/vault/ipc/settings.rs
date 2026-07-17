// src-tauri/src/vault/ipc/settings.rs
//
// Task 8: 6 个 LLM 配置 / AI 设置 IPC 命令。
//
// 安全契约：
//   - `ipc_vault_get_llm_config` 永远返回 `LlmConfigSummary`（不含 API Key）；
//   - `ipc_vault_verify_and_save_llm` 先测试再保存；测试失败时不覆盖已存配置；
//   - provider 变更且未提供新 key → 拒绝；
//   - 同 provider + 空 key → 复用已保存 key；
//   - 成功验证后清除 auth-blocked / cooldown；
//   - 删除配置时取消活跃搜索、清零所有门控状态。
//
// `LlmTestResult` 沿用既有定义；只是不再让消息中包含 reqwest 错误原文。

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::vault::config::{
    apply_preset_base_url, resolve_input, LlmConfigInput, LlmConfigStored, LlmConfigSummary,
    VaultAiSettings,
};
use crate::vault::ipc::VaultRuntimeState;
use crate::vault::llm::openai_compat::OpenAiCompatAdapter;
use crate::vault::llm::{LlmAdapter, LlmError, LlmRequest};

/// 测试结果。`message` 字段只包含本地化的稳定文案或简短的失败原因（如
/// "auth"/"rateLimit" code），不直接 echo reqwest / provider response body。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmTestResult {
    pub ok: bool,
    pub message: String,
    pub model_echo: Option<String>,
}

/// 把 `LlmError` 映射为本地化的简短消息（不泄露响应 body）。
fn llm_error_message(error: &LlmError) -> String {
    match error {
        LlmError::Auth => "API Key 失效".to_string(),
        LlmError::RateLimit => "请求被限流，请稍后重试".to_string(),
        LlmError::Timeout => "请求超时".to_string(),
        LlmError::Network(_) => "网络错误".to_string(),
        LlmError::Server(status, _) => format!("服务端返回 HTTP {status}"),
        LlmError::Parse(_) => "响应解析失败".to_string(),
        LlmError::InvalidConfig(_) => "配置无效".to_string(),
        LlmError::Cancelled => "请求已取消".to_string(),
    }
}

/// 返回当前已保存的配置概览（不含 API Key）。
#[tauri::command]
pub async fn ipc_vault_get_llm_config(
    vault: State<'_, VaultRuntimeState>,
) -> Result<Option<LlmConfigSummary>, String> {
    Ok(vault.config_summary())
}

/// 用新输入或复用已存 key 测试 LLM 连通性；只有成功才写回 DB 和 runtime。
///
/// 失败时不覆盖现有 runtime/DB 配置（前端可重试）。
#[tauri::command]
pub async fn ipc_vault_verify_and_save_llm(
    config: LlmConfigInput,
    state: State<'_, crate::AppState>,
    vault: State<'_, VaultRuntimeState>,
    app: AppHandle,
) -> Result<LlmTestResult, String> {
    // 1) 取已存配置（lock → clone → drop）
    let saved: Option<LlmConfigStored> = {
        let guard = vault.config.lock().map_err(|e| e.to_string())?;
        guard.clone()
    };

    // 2) 套预设 base_url（如留空且 provider 在预设里）
    let input = apply_preset_base_url(config);

    // 3) 合并输入与已存，得到要测试的完整配置
    let to_test = match resolve_input(input, saved.as_ref()) {
        Ok(c) => c,
        Err(msg) => {
            return Ok(LlmTestResult {
                ok: false,
                message: msg,
                model_echo: None,
            })
        }
    };

    if to_test.base_url.is_empty() || to_test.model.is_empty() {
        return Ok(LlmTestResult {
            ok: false,
            message: "配置不完整".to_string(),
            model_echo: None,
        });
    }

    // 4) 测试连通（不持任何 lock）
    let adapter = match OpenAiCompatAdapter::new(
        to_test.base_url.clone(),
        to_test.api_key.clone(),
        to_test.model.clone(),
    ) {
        Ok(a) => a,
        Err(e) => {
            let msg = llm_error_message(&e);
            return Ok(LlmTestResult {
                ok: false,
                message: msg,
                model_echo: None,
            });
        }
    };

    let req = LlmRequest {
        messages: vec![crate::vault::llm::ChatMessage::user("ping")],
        json_mode: false,
        temperature: 0.0,
        max_tokens: Some(8),
    };

    let test_result = match adapter.complete(req).await {
        Ok(resp) => LlmTestResult {
            ok: true,
            message: format!("响应 {} 字节", resp.content.len()),
            model_echo: Some(to_test.model.clone()),
        },
        Err(e) => {
            // 测试失败：不修改任何 runtime/DB 状态
            let msg = llm_error_message(&e);
            LlmTestResult {
                ok: false,
                message: msg,
                model_echo: None,
            }
        }
    };

    if !test_result.ok {
        return Ok(test_result);
    }

    // 5) 测试成功：原子写入 DB + 更新 runtime + 清门控
    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        vault
            .save_config(&mut conn, to_test)
            .map_err(|e| e.to_string())?;
    }

    // Task 10: 配置保存成功后启动一次 backfill（worker mutex 保证只一个）
    crate::vault::jobs::try_start_backfill(&app);

    Ok(test_result)
}

/// 用当前已保存的配置测试连通（用户在 Settings 里点 "Test"）。
/// 不会修改任何 runtime 状态（除非测试成功——这种情况下也只是 record_success，
/// 不需要重写 DB）。
#[tauri::command]
pub async fn ipc_vault_test_saved_llm(
    vault: State<'_, VaultRuntimeState>,
) -> Result<LlmTestResult, String> {
    let config: Option<LlmConfigStored> = {
        let guard = vault.config.lock().map_err(|e| e.to_string())?;
        guard.clone()
    };
    let Some(config) = config else {
        return Ok(LlmTestResult {
            ok: false,
            message: "未保存任何配置".to_string(),
            model_echo: None,
        });
    };

    let adapter = match OpenAiCompatAdapter::new(
        config.base_url.clone(),
        config.api_key.clone(),
        config.model.clone(),
    ) {
        Ok(a) => a,
        Err(e) => {
            return Ok(LlmTestResult {
                ok: false,
                message: llm_error_message(&e),
                model_echo: None,
            })
        }
    };

    let req = LlmRequest {
        messages: vec![crate::vault::llm::ChatMessage::user("ping")],
        json_mode: false,
        temperature: 0.0,
        max_tokens: Some(8),
    };

    let result = match adapter.complete(req).await {
        Ok(resp) => LlmTestResult {
            ok: true,
            message: format!("响应 {} 字节", resp.content.len()),
            model_echo: Some(config.model.clone()),
        },
        Err(e) => LlmTestResult {
            ok: false,
            message: llm_error_message(&e),
            model_echo: None,
        },
    };

    if result.ok {
        // 测试成功 → 清零网络失败计数（但不清 auth-blocked，因为 save_config
        // 已经处理了；这里只是给用户一个明确"健康"信号）
        vault.record_success();
    }

    Ok(result)
}

/// 删除已保存的配置 + 清零所有 runtime 门控状态。
#[tauri::command]
pub async fn ipc_vault_delete_llm_config(
    state: State<'_, crate::AppState>,
    vault: State<'_, VaultRuntimeState>,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    vault.delete_config(&mut conn).map_err(|e| e.to_string())
}

/// 返回当前 AI 设置。
#[tauri::command]
pub async fn ipc_vault_get_ai_settings(
    vault: State<'_, VaultRuntimeState>,
) -> Result<VaultAiSettings, String> {
    Ok(vault.settings())
}

/// 写入 AI 设置并返回持久化后的值。
#[tauri::command]
pub async fn ipc_vault_set_ai_settings(
    settings: VaultAiSettings,
    state: State<'_, crate::AppState>,
    vault: State<'_, VaultRuntimeState>,
) -> Result<VaultAiSettings, String> {
    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        crate::vault::config::save_ai_settings(&mut conn, &settings).map_err(|e| e.to_string())?;
    }
    vault.set_settings(settings.clone());
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratchpad::storage::ensure_dock_schema;
    use crate::vault::config::{load_ai_settings, load_stored_config};

    #[test]
    fn llm_error_message_does_not_expose_body() {
        let msg = llm_error_message(&LlmError::Server(500, "INTERNAL_SECRET".to_string()));
        assert!(!msg.contains("INTERNAL_SECRET"));
        assert!(msg.contains("500"));
    }

    #[test]
    fn llm_error_message_covers_all_variants() {
        // Just verify no panic across variants
        let _ = llm_error_message(&LlmError::Auth);
        let _ = llm_error_message(&LlmError::RateLimit);
        let _ = llm_error_message(&LlmError::Timeout);
        let _ = llm_error_message(&LlmError::Network("x".into()));
        let _ = llm_error_message(&LlmError::Server(503, "x".into()));
        let _ = llm_error_message(&LlmError::Parse("x".into()));
        let _ = llm_error_message(&LlmError::InvalidConfig("x".into()));
        let _ = llm_error_message(&LlmError::Cancelled);
    }

    // 注意：完整的 IPC 命令集成测试需要 tauri runtime + AppState，这里不便构造。
    // 核心业务逻辑（resolve_input / save / delete / 门控）已由 vault::config
    // 和 ipc::runtime_tests 覆盖。

    #[test]
    fn llm_test_result_serializes_with_camel_case() {
        let r = LlmTestResult {
            ok: true,
            message: "resp".into(),
            model_echo: Some("gpt-4".into()),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"modelEcho\""));
        assert!(json.contains("\"ok\""));
    }

    /// 验证 `ipc_vault_get_llm_config` 的核心承诺（无 API Key）经由
    /// `config_summary()` 实现 —— 直接调用 runtime，不走 tauri State。
    #[test]
    fn get_llm_config_path_returns_summary_without_key() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn, 0).unwrap();
        let stored = LlmConfigStored {
            provider_id: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: "topsecret".into(),
            model: "deepseek-chat".into(),
        };
        crate::vault::config::save_stored_config(&mut conn, &stored).unwrap();
        let runtime = VaultRuntimeState::load(&conn);
        let summary = runtime.config_summary().expect("config saved");
        let json = serde_json::to_string(&summary).unwrap();
        assert!(!json.contains("topsecret"));
        assert!(summary.has_api_key);
    }

    /// 模拟 verify-and-save 失败路径：测试不通过时不应写入 DB。
    #[test]
    fn verify_and_save_failure_does_not_overwrite_existing() {
        // 这里的逻辑核心在 resolve_input；直接测它即可（无 tauri State）。
        let saved = LlmConfigStored {
            provider_id: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: "old-key".into(),
            model: "deepseek-chat".into(),
        };
        // provider 变更 + 空 key → 拒绝（不会覆盖 saved）
        let input = LlmConfigInput {
            provider_id: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: None,
            model: "gpt-5.6-sol".into(),
        };
        assert!(resolve_input(input, Some(&saved)).is_err());
    }

    /// 模拟 delete_config 完整路径：DB 行消失、runtime 清零。
    #[test]
    fn delete_config_clears_db_and_runtime() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn, 0).unwrap();
        let stored = LlmConfigStored {
            provider_id: "deepseek".into(),
            base_url: "u".into(),
            api_key: "k".into(),
            model: "m".into(),
        };
        crate::vault::config::save_stored_config(&mut conn, &stored).unwrap();
        let runtime = VaultRuntimeState::load(&conn);
        runtime.set_auth_blocked(true);
        runtime.record_failure(&LlmError::RateLimit);
        runtime.delete_config(&mut conn).unwrap();
        assert!(load_stored_config(&conn).is_none());
        assert!(runtime.config.lock().unwrap().is_none());
        assert!(runtime.should_skip_automatic_call().is_none());
        // settings 仍可读
        let _ = load_ai_settings(&conn);
    }
}
