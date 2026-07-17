// src-tauri/src/vault/config.rs
//
// Task 8: LLM 配置类型、AI 设置以及对应的 SQLite 持久化助手。
//
// 所有"用户可保存/可获取"的配置都在这里定型：
//   - `LlmConfigStored`  —— 数据库里持久化的完整配置（含明文 API Key）。
//   - `LlmConfigInput`   —— 前端调用 verify-and-save 时传入的输入；
//     `api_key` 是 `Option<String>`：`None` 或空字符串表示"复用已保存值"。
//   - `LlmConfigSummary` —— 返回前端的只读视图，**绝不包含 API Key**，
//     只有 `has_api_key: bool` 标志位。
//   - `VaultAiSettings`  —— 用户可调的 AI 行为开关（auto_enrich、
//     auto_hybrid_search、sensitive_clipboard_clear_seconds），默认
//     `{ true, true, Some(30) }`。
//
// 持久化使用 `preferences` 表，与现有 dock preferences 共表，键名固定为
// `vault_llm_config` / `vault_ai_settings`。两个值都用 JSON 序列化。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::storage::error::{StorageError, StorageResult};
use crate::vault::llm::presets::find_preset;

pub const LLM_CONFIG_PREF_KEY: &str = "vault_llm_config";
pub const AI_SETTINGS_PREF_KEY: &str = "vault_ai_settings";

/// 数据库里持久化的完整 LLM 配置（含明文 API Key）。
///
/// 只在 backend 内部出现；前端永远拿不到这种类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfigStored {
    pub provider_id: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

/// 前端 verify-and-save 时传入的输入。
///
/// `api_key` 是 `Option<String>`：
/// - `Some(s)` 当 `s` 非空 → 用新 key 测试并保存；
/// - `Some("")` 或 `None` → 复用已保存 key（如果 provider 没变）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfigInput {
    pub provider_id: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

/// 返回前端的只读配置概览。
///
/// **绝不包含 API Key**，只有 `has_api_key: bool`。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfigSummary {
    pub provider_id: String,
    pub base_url: String,
    pub model: String,
    pub has_api_key: bool,
}

impl LlmConfigStored {
    pub fn summary(&self) -> LlmConfigSummary {
        LlmConfigSummary {
            provider_id: self.provider_id.clone(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            has_api_key: !self.api_key.is_empty(),
        }
    }
}

/// 用户可调的 AI 行为开关。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultAiSettings {
    pub auto_enrich: bool,
    pub auto_hybrid_search: bool,
    pub sensitive_clipboard_clear_seconds: Option<u64>,
}

impl Default for VaultAiSettings {
    fn default() -> Self {
        Self {
            auto_enrich: true,
            auto_hybrid_search: true,
            sensitive_clipboard_clear_seconds: Some(30),
        }
    }
}

// ---- 持久化助手 -------------------------------------------------------------

/// 从 `preferences` 表读取已保存的 LLM 配置。不存在时返回 `None`。
pub fn load_stored_config(conn: &Connection) -> Option<LlmConfigStored> {
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM preferences WHERE key=?1",
            params![LLM_CONFIG_PREF_KEY],
            |r| r.get(0),
        )
        .ok();
    v.and_then(|s| serde_json::from_str(&s).ok())
}

/// 写入 LLM 配置（UPSERT）。
pub fn save_stored_config(conn: &mut Connection, cfg: &LlmConfigStored) -> StorageResult<()> {
    let s = serde_json::to_string(cfg).map_err(|e| StorageError::Other(e.to_string()))?;
    conn.execute(
        "INSERT INTO preferences(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![LLM_CONFIG_PREF_KEY, s],
    )?;
    Ok(())
}

/// 删除已保存的 LLM 配置。不存在时也是 no-op。
pub fn delete_stored_config(conn: &mut Connection) -> StorageResult<()> {
    conn.execute(
        "DELETE FROM preferences WHERE key=?1",
        params![LLM_CONFIG_PREF_KEY],
    )?;
    Ok(())
}

/// 读取 AI 设置；不存在或解析失败时回退到默认值（保证 runtime 始终可用）。
pub fn load_ai_settings(conn: &Connection) -> VaultAiSettings {
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM preferences WHERE key=?1",
            params![AI_SETTINGS_PREF_KEY],
            |r| r.get(0),
        )
        .ok();
    v.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 写入 AI 设置（UPSERT）。
pub fn save_ai_settings(conn: &mut Connection, settings: &VaultAiSettings) -> StorageResult<()> {
    let s = serde_json::to_string(settings).map_err(|e| StorageError::Other(e.to_string()))?;
    conn.execute(
        "INSERT INTO preferences(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![AI_SETTINGS_PREF_KEY, s],
    )?;
    Ok(())
}

/// 当 base_url 留空且 provider 在预设里时，回填预设 base_url。
/// 返回值用于后续测试 / 保存。
pub fn apply_preset_base_url(mut input: LlmConfigInput) -> LlmConfigInput {
    if input.base_url.is_empty() {
        if let Some(p) = find_preset(&input.provider_id) {
            input.base_url = p.base_url.to_string();
        }
    }
    input
}

/// 把 `LlmConfigInput` 与（可能存在的）已保存配置合并，解析出最终要测试
/// 与保存的 `LlmConfigStored`。
///
/// 规则：
/// - `api_key` 非空（`Some(s)` 且 `s` 不为空）→ 用新 key；
/// - `api_key` 空 → 复用 `saved.api_key`；
/// - `api_key` 空且无 saved → 返回 `Err`（缺 key）；
/// - `api_key` 空且 provider_id 与 saved 不一致 → 返回 `Err`（换 provider
///   必须重新提供 key）。
///
/// 返回 `Ok(Some(stored))` 表示可继续测试；返回 `Ok(None)` 表示 saved
/// 不存在且 input 没填 key（用于首次配置时给出明确错误）——调用方需要
/// 自己处理这两种情况。这里简单以 `Err(String)` 表达所有"无法合并"。
pub fn resolve_input(
    input: LlmConfigInput,
    saved: Option<&LlmConfigStored>,
) -> Result<LlmConfigStored, String> {
    let provider_id = input.provider_id.clone();
    let base_url = input.base_url.clone();
    let model = input.model.clone();

    let api_key = match input.api_key.as_deref() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            // 空 key → 必须复用 saved
            match saved {
                Some(s) => {
                    if s.provider_id != provider_id {
                        return Err("切换 provider 时必须重新填写 API Key".to_string());
                    }
                    s.api_key.clone()
                }
                None => return Err("API Key 不能为空".to_string()),
            }
        }
    };

    Ok(LlmConfigStored {
        provider_id,
        base_url,
        api_key,
        model,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratchpad::storage::ensure_dock_schema;

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

    // ---- LlmConfigStored.summary ------------------------------------------

    #[test]
    fn summary_never_returns_api_key() {
        let stored = sample_stored();
        let s = stored.summary();
        // 序列化后的字符串不应包含明文 key
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("sk-secret"));
        assert!(s.has_api_key);
        assert_eq!(s.provider_id, "deepseek");
        assert_eq!(s.model, "deepseek-chat");
    }

    #[test]
    fn summary_reports_has_api_key_false_when_empty() {
        let stored = LlmConfigStored {
            provider_id: "x".into(),
            base_url: "u".into(),
            api_key: "".into(),
            model: "m".into(),
        };
        assert!(!stored.summary().has_api_key);
    }

    // ---- VaultAiSettings default & round-trip ------------------------------

    #[test]
    fn default_ai_settings_has_true_true_thirty() {
        let s = VaultAiSettings::default();
        assert!(s.auto_enrich);
        assert!(s.auto_hybrid_search);
        assert_eq!(s.sensitive_clipboard_clear_seconds, Some(30));
    }

    #[test]
    fn ai_settings_roundtrip_persists_values() {
        let mut conn = open_db();
        let s = VaultAiSettings {
            auto_enrich: false,
            auto_hybrid_search: true,
            sensitive_clipboard_clear_seconds: Some(90),
        };
        save_ai_settings(&mut conn, &s).unwrap();
        let loaded = load_ai_settings(&conn);
        assert!(!loaded.auto_enrich);
        assert!(loaded.auto_hybrid_search);
        assert_eq!(loaded.sensitive_clipboard_clear_seconds, Some(90));
    }

    #[test]
    fn load_ai_settings_returns_default_when_missing() {
        let conn = open_db();
        let s = load_ai_settings(&conn);
        assert!(s.auto_enrich);
        assert_eq!(s.sensitive_clipboard_clear_seconds, Some(30));
    }

    #[test]
    fn load_ai_settings_falls_back_when_corrupt() {
        let conn = open_db();
        conn.execute(
            "INSERT INTO preferences(key, value) VALUES (?1, 'not-json')",
            params![AI_SETTINGS_PREF_KEY],
        )
        .unwrap();
        // 回退到默认，不 panic
        let s = load_ai_settings(&conn);
        assert!(s.auto_enrich);
    }

    // ---- LlmConfigStored persistence --------------------------------------

    #[test]
    fn load_returns_none_when_missing() {
        let conn = open_db();
        assert!(load_stored_config(&conn).is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let mut conn = open_db();
        let cfg = sample_stored();
        save_stored_config(&mut conn, &cfg).unwrap();
        let loaded = load_stored_config(&conn).unwrap();
        assert_eq!(loaded.provider_id, cfg.provider_id);
        assert_eq!(loaded.api_key, cfg.api_key);
    }

    #[test]
    fn delete_removes_config() {
        let mut conn = open_db();
        let cfg = sample_stored();
        save_stored_config(&mut conn, &cfg).unwrap();
        assert!(load_stored_config(&conn).is_some());
        delete_stored_config(&mut conn).unwrap();
        assert!(load_stored_config(&conn).is_none());
    }

    // ---- resolve_input ----------------------------------------------------

    #[test]
    fn resolve_input_uses_new_key_when_provided() {
        let input = LlmConfigInput {
            provider_id: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: Some("sk-new".into()),
            model: "deepseek-chat".into(),
        };
        let stored = resolve_input(input, None).unwrap();
        assert_eq!(stored.api_key, "sk-new");
    }

    #[test]
    fn resolve_input_reuses_saved_key_when_blank_same_provider() {
        let saved = sample_stored();
        let input = LlmConfigInput {
            provider_id: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: None,
            model: "deepseek-v4-pro".into(),
        };
        let stored = resolve_input(input, Some(&saved)).unwrap();
        assert_eq!(stored.api_key, "sk-secret");
        assert_eq!(stored.model, "deepseek-v4-pro"); // 来自 input
    }

    #[test]
    fn resolve_input_rejects_blank_key_when_provider_changed() {
        let saved = sample_stored();
        let input = LlmConfigInput {
            provider_id: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: None,
            model: "gpt-5.6-sol".into(),
        };
        assert!(resolve_input(input, Some(&saved)).is_err());
    }

    #[test]
    fn resolve_input_rejects_blank_key_when_no_saved() {
        let input = LlmConfigInput {
            provider_id: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: None,
            model: "deepseek-chat".into(),
        };
        assert!(resolve_input(input, None).is_err());
    }

    #[test]
    fn resolve_input_rejects_empty_string_key_when_no_saved() {
        let input = LlmConfigInput {
            provider_id: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: Some("".into()),
            model: "deepseek-chat".into(),
        };
        assert!(resolve_input(input, None).is_err());
    }

    #[test]
    fn apply_preset_fills_base_url_for_known_provider() {
        let input = LlmConfigInput {
            provider_id: "deepseek".into(),
            base_url: "".into(),
            api_key: Some("k".into()),
            model: "deepseek-chat".into(),
        };
        let out = apply_preset_base_url(input);
        assert_eq!(out.base_url, "https://api.deepseek.com/v1");
    }
}
