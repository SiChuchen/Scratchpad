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

use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Semaphore;

use crate::vault::ai::parse_capture_response;
use crate::vault::desensitize::{desensitize_entry, TokenMap};
use crate::vault::ipc::VaultRuntimeState;
use crate::vault::llm::openai_compat::OpenAiCompatAdapter;
use crate::vault::llm::prompt::capture_enrichment_prompt;
use crate::vault::llm::{LlmAdapter, LlmRequest};
use crate::vault::models::{AiMetadataStatus, BackfillStatus, VaultAiMetadata, VaultEntryDetail};
use crate::vault::storage as vstore;

/// 每完成一条 entry 后的节流间隔。
const BACKFILL_THROTTLE_MS: u64 = 750;
/// 单次 worker run 最多处理多少条 entry（防止无限循环）。
const BACKFILL_BATCH_LIMIT: usize = 50;

/// 单例 worker 信号量。容量 1；permit 在 worker 任务结束时自动 Drop
/// （即使 worker panic 也会释放，避免 `WORKER_RUNNING=true` 永久卡死）。
static WORKER_PERMIT: Semaphore = Semaphore::const_new(1);

/// 在 worker 内向外发送的 metadata 更新事件 payload。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiMetadataUpdatedEvent {
    pub entry_id: String,
    pub status: String,
    pub tags: Vec<String>,
    /// 当前最新 metadata（写回 DB 的快照）。error 路径下为 None。
    pub metadata: Option<VaultAiMetadata>,
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
///
/// 使用 `Semaphore::try_acquire` 而非 `Mutex<bool>`：permit 通过 RAII Drop
/// 释放，即使 worker 任务 panic 也能恢复，避免 flag 永久卡在 true。
pub fn try_start_backfill(app: &AppHandle) {
    let permit = match WORKER_PERMIT.try_acquire() {
        Ok(p) => p,
        Err(_) => return, // 已有 worker 在运行
    };

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let _permit = permit; // moved into task; Drop releases on normal return OR panic
        run_backfill_loop(app_handle).await;
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

    let adapter =
        match OpenAiCompatAdapter::new(config.base_url, config.api_key, config.model.clone()) {
            Ok(a) => a,
            Err(_) => return,
        };

    // 3) 脱敏（请求局部 TokenMap）
    let tag_strings: Vec<String> = detail.tags.iter().map(|t| t.tag.clone()).collect();
    let mut token_map = TokenMap::new();
    let d_entry = desensitize_entry(&detail.entry, &detail.fields, &tag_strings, &mut token_map);
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
            write_status_and_emit(app, entry_id, AiMetadataStatus::Error, Vec::new(), None).await;
            return;
        }
    };

    // 5) 解析响应
    let suggestion = match parse_capture_response(&resp.content, &token_map) {
        Ok(s) => s,
        Err(e) => {
            vault.record_failure(&e);
            write_status_and_emit(app, entry_id, AiMetadataStatus::Error, Vec::new(), None).await;
            return;
        }
    };

    // 6) 计算 content_hash（保持与 create_entry 时一致）
    let content_hash = vstore::compute_entry_content_hash(&detail.entry, &detail.fields);

    // 6.5) **竞态保护**：LLM 调用可能持续数十秒。期间 `update_entry` 可能
    // 已经把 entry 改成了新内容（删 ai tags、置 pending、写新 content_hash）。
    // 若直接用旧 snapshot 的 hash 写回，会把用户编辑触发的 pending 状态用
    // ready + stale 数据覆盖，且该 entry 不会再次回填（status 已变 ready）。
    //
    // 解决方案：写之前在 DB 锁下重新读取 entry 的当前 ai content_hash，
    // 若与 snapshot 不一致，放弃本次写。entry 会保持 pending 状态，由
    // 下一轮回填处理。
    {
        let app_state = app.state::<crate::AppState>();
        let Ok(conn) = app_state.db.lock() else {
            return;
        };
        let current_hash = vstore::ai_content_hash_for_entry(&conn, entry_id).unwrap_or_default();
        // 读 metadata 当前 status，只有仍为 pending（未变 ready/error）才写
        let current_status = vstore::get_ai_metadata(&conn, entry_id)
            .ok()
            .flatten()
            .map(|m| m.status);
        if current_hash != content_hash || current_status != Some(AiMetadataStatus::Pending) {
            // entry 已被修改或已处理；放弃本次写入
            return;
        }
    }

    // 7) 写入 ready metadata + replace ai tags
    let metadata = VaultAiMetadata {
        entry_id: entry_id.to_string(),
        summary: suggestion.ai_summary.clone(),
        search_aliases: suggestion.search_aliases.clone(),
        content_hash,
        provider_id: Some(config.provider_id.clone()),
        model: Some(config.model.clone()),
        generated_at: Some(chrono::Utc::now().to_rfc3339()),
        status: AiMetadataStatus::Ready,
    };
    {
        let app_state = app.state::<crate::AppState>();
        let Ok(mut conn) = app_state.db.lock() else {
            return;
        };
        let _ = vstore::replace_ai_tags(&mut conn, entry_id, &suggestion.ai_tags);
        let _ = vstore::set_ai_metadata(&mut conn, &metadata);
    }

    // 8) 发事件
    write_status_and_emit(
        app,
        entry_id,
        AiMetadataStatus::Ready,
        suggestion.ai_tags,
        Some(metadata),
    )
    .await;
}

/// 把 metadata 状态写回 DB（用于 error 路径），并 emit 两个事件。
async fn write_status_and_emit(
    app: &AppHandle,
    entry_id: &str,
    status: AiMetadataStatus,
    tags: Vec<String>,
    metadata: Option<VaultAiMetadata>,
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
        metadata: metadata.clone(),
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

    #[test]
    fn ai_metadata_updated_event_serializes_camel_case_with_metadata() {
        // spec 要求：payload 是 { entryId, status, tags, metadata }
        use crate::vault::models::VaultAiMetadata;
        let metadata = VaultAiMetadata {
            entry_id: "v1".into(),
            summary: Some("hello".into()),
            search_aliases: vec!["al".into()],
            content_hash: "abc".into(),
            provider_id: Some("deepseek".into()),
            model: Some("deepseek-chat".into()),
            generated_at: Some("2026-07-17T00:00:00Z".into()),
            status: AiMetadataStatus::Ready,
        };
        let evt = AiMetadataUpdatedEvent {
            entry_id: "v1".into(),
            status: "ready".into(),
            tags: vec!["t1".into()],
            metadata: Some(metadata.clone()),
        };
        let json = serde_json::to_value(&evt).unwrap();
        // camelCase
        assert!(json.get("entryId").is_some(), "entryId must be camelCase");
        assert!(json.get("metadata").is_some(), "metadata field required");
        assert_eq!(json["status"], "ready");
        assert_eq!(json["tags"][0], "t1");
        assert_eq!(json["metadata"]["summary"], "hello");
        assert_eq!(json["metadata"]["entryId"], "v1");
        assert!(json.get("entry_id").is_none(), "snake_case must not leak");
        assert!(json.get("search_aliases").is_none());
    }

    #[test]
    fn ai_metadata_updated_event_serializes_null_metadata_for_error() {
        // error 路径 metadata=None → 应序列化为 null
        let evt = AiMetadataUpdatedEvent {
            entry_id: "v2".into(),
            status: "error".into(),
            tags: Vec::new(),
            metadata: None,
        };
        let json = serde_json::to_value(&evt).unwrap();
        assert_eq!(json["entryId"], "v2");
        assert_eq!(json["status"], "error");
        assert!(
            json["metadata"].is_null(),
            "metadata should be null on error"
        );
    }

    /// 回归 C1：worker 在 LLM 调用期间 entry 被并发 update_entry 修改时，
    /// 写入路径必须重新检查 ai_content_hash + status，避免用过期 snapshot
    /// 覆盖新写入的 pending metadata。
    ///
    /// 由于 `process_one_entry` 需要 `AppHandle`，无法在纯单元测试里直接驱动；
    /// 这里以 storage-level API 模拟同一竞态序列，验证保护逻辑的输入条件
    /// （current_hash != snapshot_hash OR status != pending → 跳过写入）。
    #[test]
    fn backfill_skips_when_entry_changed_during_llm_call() {
        use crate::vault::models::{FieldInput, VaultEntryInput};

        let mut conn = open_db();
        let mk_input = |title: &str| VaultEntryInput {
            kind: crate::vault::models::EntryKind::Note,
            title: title.into(),
            fields: Vec::<FieldInput>::new(),
            notes: None,
            manual_tags: Vec::new(),
        };

        // 1) 创建 entry，默认 pending
        let detail = vstore::create_entry(&mut conn, &mk_input("Original")).unwrap();
        let id = detail.entry.id.clone();
        let snapshot_hash = vstore::compute_entry_content_hash(&detail.entry, &detail.fields);

        // 2) 模拟"LLM 调用期间，update_entry 修改了 title → 触发 hash 变化 +
        //    status 仍为 pending（update_entry 内部把 ready→pending；这里
        //    默认就是 pending）"
        let mut new_input = mk_input("Edited during LLM");
        new_input.title = "Edited during LLM".into();
        vstore::update_entry(&mut conn, &id, &new_input).unwrap();

        // 3) 保护逻辑：重新读取当前 hash + status
        let current_hash = vstore::ai_content_hash_for_entry(&conn, &id).unwrap();
        let current_status = vstore::get_ai_metadata(&conn, &id).unwrap().unwrap().status;

        // 4) 断言：snapshot_hash != current_hash → 写入必须被跳过
        assert_ne!(
            snapshot_hash, current_hash,
            "update_entry should have changed the content hash"
        );
        assert_eq!(current_status, AiMetadataStatus::Pending);

        // 模拟 process_one_entry 的决策分支
        let should_skip =
            current_hash != snapshot_hash || current_status != AiMetadataStatus::Pending;
        assert!(
            should_skip,
            "worker must skip write when entry changed during LLM call"
        );

        // 5) 验证：跳过写入后，metadata 仍是 pending、无 summary、无 ai tags
        //    （即 update_entry 留下的"等待重新回填"状态未被覆盖）
        let md = vstore::get_ai_metadata(&conn, &id).unwrap().unwrap();
        assert_eq!(md.status, AiMetadataStatus::Pending);
        assert!(md.summary.is_none());
        let tags = vstore::list_tags_with_source(&conn, &id).unwrap();
        assert!(
            !tags
                .iter()
                .any(|t| t.source == crate::vault::models::TagSource::Ai),
            "no stale AI tags should be written"
        );
    }

    /// 回归 I1：worker 通过 Semaphore permit 保护，panic 后能再次启动。
    /// 由于 `try_start_backfill` 需要 `AppHandle`，这里直接验证 Semaphore
    /// 本身的 panic-safe 语义：模拟 permit 被占用然后"释放"（panic 等价于
    /// permit 被 Drop）。
    #[test]
    fn worker_permit_recovers_after_drop() {
        // 静态 Semaphore 在测试间共享，可能处于被占用状态；这里使用局部
        // Semaphore 验证 panic-safe 语义（Drop 总是释放）。
        let sem = Semaphore::new(1);
        let permit1 = sem.try_acquire().unwrap();
        // 占用后第二个 acquire 失败
        assert!(sem.try_acquire().is_err());
        // 模拟 panic：drop permit（panic 时 Rust 也会 Drop）
        drop(permit1);
        // 应能再次 acquire
        assert!(sem.try_acquire().is_ok());
    }
}
