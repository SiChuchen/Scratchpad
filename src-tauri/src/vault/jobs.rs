// src-tauri/src/vault/jobs.rs
//
// Task 10: 串行后台回填 worker。
//
// 设计：
//   * 同一时间最多一个 worker（用 `worker_running: Mutex<bool>` 保证）；
//   * 每完成一条 entry 后 sleep 750ms（节流）；
//   * 终止条件：config 被删、auto_enrich 关闭、auth_blocked、app 退出；
//   * 每条结束发两个事件：
//       - `vault-ai-metadata-updated { entryId, status, tags, metadata }`
//       - `vault-ai-backfill-progress { total, pending, processing, ready, error }`
//
// 触发时机：
//   * app setup：若 config 存在且 auto_enrich=true，spawn 一个 worker；
//   * verify_and_save 成功后：再次 trigger（worker mutex 保证重复触发是 no-op）。

use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::vault::ai::parse_capture_response;
use crate::vault::desensitize::{desensitize_entry, TokenMap};
use crate::vault::ipc::VaultRuntimeState;
use crate::vault::llm::openai_compat::OpenAiCompatAdapter;
use crate::vault::llm::prompt::capture_enrichment_prompt;
use crate::vault::llm::{LlmAdapter, LlmRequest};
use crate::vault::models::{
    AiMetadataStatus, BackfillStatus, VaultAiMetadata, VaultEntryDetail,
};
use crate::vault::storage as vstore;

/// 每完成一条 entry 后的节流间隔。
const BACKFILL_THROTTLE_MS: u64 = 750;
/// 单次 worker run 最多处理多少条 entry（防止无限循环）。
const BACKFILL_BATCH_LIMIT: usize = 50;

/// 单例 worker mutex。`true` 表示已有 worker 在运行。
static WORKER_RUNNING: Mutex<bool> = Mutex::new(false);

/// 在 worker 内向外发送的 metadata 更新事件 payload。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiMetadataUpdatedEvent {
    pub entry_id: String,
    pub status: String,
    pub tags: Vec<String>,
}

/// 判断"当前是否应当启动 backfill worker"。
///
/// 条件：config 存在 AND auto_enrich=true AND 未被 auth-blocked。
/// 注意：cooldown 不会阻止启动 worker —— worker 在每条 entry 内部自行
/// 通过 `should_skip_automatic_call` 检查；这里只检查持久性约束。
pub fn should_run_backfill(runtime: &VaultRuntimeState) -> bool {
    let has_config = runtime.config.lock().unwrap().is_some();
    if !has_config {
        return false;
    }
    let settings = runtime.settings();
    if !settings.auto_enrich {
        return false;
    }
    if runtime.is_auth_blocked_pub() {
        return false;
    }
    true
}

/// 尝试启动一个 backfill worker。如果已有 worker 在运行，直接返回（不排队）。
///
/// 调用方通常在 setup 或 verify_and_save 成功后调用。
pub fn try_start_backfill(app: &AppHandle) {
    // 单 worker 检查
    {
        let mut guard = WORKER_RUNNING.lock().unwrap();
        if *guard {
            return;
        }
        *guard = true;
    }

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        run_backfill_loop(app_handle).await;
        // 离开 loop 时清掉 running flag
        let mut guard = WORKER_RUNNING.lock().unwrap();
        *guard = false;
    });
}

/// Worker 主循环：每次取一批 pending entries，逐条处理，期间检查终止条件。
async fn run_backfill_loop(app: AppHandle) {
    loop {
        // 终止条件：config 删除 / auto_enrich 关闭 / auth_blocked
        let vault = app.state::<VaultRuntimeState>();
        if !should_run_backfill(&vault) {
            return;
        }

        // 取一批 pending
        let entries: Vec<String> = {
            let app_state = app.state::<crate::AppState>();
            let conn = match app_state.db.lock() {
                Ok(c) => c,
                Err(_) => return,
            };
            match vstore::list_pending_ai_entries(&conn, BACKFILL_BATCH_LIMIT) {
                Ok(v) => v,
                Err(_) => return,
            }
        };

        if entries.is_empty() {
            // 没有更多 pending，结束本轮 worker
            return;
        }

        for entry_id in entries {
            // 每条开始前再次检查终止条件
            let vault = app.state::<VaultRuntimeState>();
            if !should_run_backfill(&vault) {
                return;
            }
            // 门控：auth-blocked 或 cooldown → 跳过本条但继续循环
            if vault.should_skip_automatic_call().is_some() {
                continue;
            }

            process_one_entry(&app, &entry_id).await;

            // 节流：每条结束后 750ms
            tokio::time::sleep(Duration::from_millis(BACKFILL_THROTTLE_MS)).await;
        }
    }
}

/// 处理单条 entry：脱敏 → 调 LLM → 解析 → 写回 metadata + emit events。
async fn process_one_entry(app: &AppHandle, entry_id: &str) {
    // 1) 取 entry 详情（lock db → 取 → drop guard）
    let detail: Option<VaultEntryDetail> = {
        let app_state = app.state::<crate::AppState>();
        let conn = match app_state.db.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        vstore::get_entry_detail(&conn, entry_id).ok()
    };
    let Some(detail) = detail else { return };

    // 2) 取 config + adapter
    let vault = app.state::<VaultRuntimeState>();
    let config = {
        let guard = vault.config.lock().unwrap();
        guard.clone()
    };
    let Some(config) = config else { return };

    let adapter = match OpenAiCompatAdapter::new(
        config.base_url,
        config.api_key,
        config.model.clone(),
    ) {
        Ok(a) => a,
        Err(_) => return,
    };

    // 3) 脱敏（请求局部 TokenMap）
    let tag_strings: Vec<String> = detail.tags.iter().map(|t| t.tag.clone()).collect();
    let mut token_map = TokenMap::new();
    let d_entry = desensitize_entry(
        &detail.entry,
        &detail.fields,
        &tag_strings,
        &mut token_map,
    );
    let mut masked_text = String::new();
    masked_text.push_str("title: ");
    masked_text.push_str(&d_entry.title);
    masked_text.push('\n');
    if !d_entry.notes.is_empty() {
        masked_text.push_str("notes: ");
        masked_text.push_str(&d_entry.notes);
        masked_text.push('\n');
    }
    for f in &d_entry.fields {
        masked_text.push_str(&format!("{}: {}\n", f.key, f.value));
    }
    if !d_entry.tags.is_empty() {
        masked_text.push_str(&format!("tags: {}\n", d_entry.tags.join(", ")));
    }

    // 4) 组装 prompt + 调 LLM
    let messages = capture_enrichment_prompt(&masked_text);
    let req = LlmRequest {
        messages,
        json_mode: true,
        temperature: 0.3,
        max_tokens: Some(512),
    };
    let resp = match adapter.complete(req).await {
        Ok(r) => {
            vault.record_success();
            r
        }
        Err(e) => {
            vault.record_failure(&e);
            // 失败：把 metadata 置为 error 并发事件
            write_status_and_emit(app, entry_id, AiMetadataStatus::Error, Vec::new()).await;
            return;
        }
    };

    // 5) 解析响应
    let suggestion = match parse_capture_response(&resp.content, &token_map) {
        Ok(s) => s,
        Err(e) => {
            vault.record_failure(&e);
            write_status_and_emit(app, entry_id, AiMetadataStatus::Error, Vec::new()).await;
            return;
        }
    };

    // 6) 计算 content_hash（保持与 create_entry 时一致）
    let content_hash = vstore::compute_entry_content_hash(&detail.entry, &detail.fields);

    // 7) 写入 ready metadata + replace ai tags
    {
        let app_state = app.state::<crate::AppState>();
        let Ok(mut conn) = app_state.db.lock() else { return };
        let _ = vstore::replace_ai_tags(&mut conn, entry_id, &suggestion.ai_tags);
        let _ = vstore::set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: entry_id.to_string(),
                summary: suggestion.ai_summary.clone(),
                search_aliases: suggestion.search_aliases.clone(),
                content_hash,
                provider_id: Some(config.provider_id.clone()),
                model: Some(config.model.clone()),
                generated_at: Some(chrono::Utc::now().to_rfc3339()),
                status: AiMetadataStatus::Ready,
            },
        );
    }

    // 8) 发事件
    write_status_and_emit(app, entry_id, AiMetadataStatus::Ready, suggestion.ai_tags).await;
}

/// 把 metadata 状态写回 DB（用于 error 路径），并 emit 两个事件。
async fn write_status_and_emit(
    app: &AppHandle,
    entry_id: &str,
    status: AiMetadataStatus,
    tags: Vec<String>,
) {
    if status == AiMetadataStatus::Error {
        // 把 metadata 标 error
        let app_state = app.state::<crate::AppState>();
        let lock_result = app_state.db.lock();
        if let Ok(mut conn) = lock_result {
            let _ = mark_metadata_error(&mut conn, entry_id);
        }
    }

    // emit metadata-updated
    let payload = AiMetadataUpdatedEvent {
        entry_id: entry_id.to_string(),
        status: status.as_str().to_string(),
        tags: tags.clone(),
    };
    let _ = app.emit("vault-ai-metadata-updated", payload);

    // emit progress
    let progress: BackfillStatus = {
        let app_state = app.state::<crate::AppState>();
        let conn_guard = app_state.db.lock();
        match conn_guard {
            Ok(c) => vstore::backfill_status(&c).unwrap_or_default(),
            Err(_) => BackfillStatus::default(),
        }
    };
    let _ = app.emit("vault-ai-backfill-progress", progress);
}

/// 把 entry 的 metadata status 置为 error（保留 content_hash 不变）。
fn mark_metadata_error(conn: &mut Connection, entry_id: &str) -> Result<(), String> {
    // 简单 update：若不存在 metadata 行，忽略（pending 时还没创建则无所谓）
    conn.execute(
        "UPDATE vault_ai_metadata SET status='error' WHERE entry_id=?1",
        rusqlite::params![entry_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ---- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratchpad::storage::ensure_dock_schema;
    use crate::vault::config::{save_ai_settings, save_stored_config, VaultAiSettings};
    use rusqlite::Connection;

    fn open_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn, 0).unwrap();
        vstore::ensure_vault_schema(&mut conn).unwrap();
        conn
    }

    fn sample_stored() -> crate::vault::config::LlmConfigStored {
        crate::vault::config::LlmConfigStored {
            provider_id: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: "sk-secret".into(),
            model: "deepseek-chat".into(),
        }
    }

    #[test]
    fn should_run_backfill_requires_config_and_auto_enrich() {
        let mut conn = open_db();
        save_stored_config(&mut conn, &sample_stored()).unwrap();
        save_ai_settings(
            &mut conn,
            &VaultAiSettings {
                auto_enrich: true,
                auto_hybrid_search: true,
                sensitive_clipboard_clear_seconds: Some(30),
            },
        )
        .unwrap();
        let runtime = VaultRuntimeState::load(&conn);
        assert!(should_run_backfill(&runtime));
    }

    #[test]
    fn should_not_run_backfill_when_auto_enrich_disabled() {
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
        assert!(!should_run_backfill(&runtime));
    }

    #[test]
    fn should_not_run_backfill_without_config() {
        let mut conn = open_db();
        save_ai_settings(
            &mut conn,
            &VaultAiSettings {
                auto_enrich: true,
                auto_hybrid_search: true,
                sensitive_clipboard_clear_seconds: Some(30),
            },
        )
        .unwrap();
        let runtime = VaultRuntimeState::load(&conn);
        assert!(!should_run_backfill(&runtime));
    }

    #[test]
    fn should_not_run_backfill_when_auth_blocked() {
        let mut conn = open_db();
        save_stored_config(&mut conn, &sample_stored()).unwrap();
        save_ai_settings(
            &mut conn,
            &VaultAiSettings {
                auto_enrich: true,
                auto_hybrid_search: true,
                sensitive_clipboard_clear_seconds: Some(30),
            },
        )
        .unwrap();
        let runtime = VaultRuntimeState::load(&conn);
        runtime.set_auth_blocked(true);
        assert!(!should_run_backfill(&runtime));
    }

    #[test]
    fn backfill_status_query_returns_correct_counts() {
        // 复用 capture.rs 的同一组断言，但这里直接验证 storage::backfill_status
        let mut conn = open_db();
        use crate::vault::models::{
            AiMetadataStatus, FieldInput, VaultAiMetadata, VaultEntryInput,
        };
        let mk_input = |title: &str| VaultEntryInput {
            kind: crate::vault::models::EntryKind::Note,
            title: title.into(),
            fields: Vec::<FieldInput>::new(),
            notes: None,
            manual_tags: Vec::new(),
        };
        let r1 = vstore::create_entry(&mut conn, &mk_input("r1")).unwrap();
        let _p1 = vstore::create_entry(&mut conn, &mk_input("p1")).unwrap();
        let e1 = vstore::create_entry(&mut conn, &mk_input("e1")).unwrap();
        vstore::set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: r1.entry.id,
                summary: None,
                search_aliases: Vec::new(),
                content_hash: "h1".into(),
                provider_id: None,
                model: None,
                generated_at: None,
                status: AiMetadataStatus::Ready,
            },
        )
        .unwrap();
        vstore::set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: e1.entry.id,
                summary: None,
                search_aliases: Vec::new(),
                content_hash: "h2".into(),
                provider_id: None,
                model: None,
                generated_at: None,
                status: AiMetadataStatus::Error,
            },
        )
        .unwrap();
        let s = vstore::backfill_status(&conn).unwrap();
        assert_eq!(s.total, 3);
        assert_eq!(s.ready, 1);
        assert_eq!(s.pending, 1);
        assert_eq!(s.error, 1);
    }
}
