// src-tauri/src/vault/ipc/entries.rs
//
// Task 10: Vault entry CRUD + manual tags + AI metadata refresh + backfill
// status IPC commands.
//
// 行为契约：
//   * `create_entry` / `update_entry` 在 DB 中保存 pending metadata（视内容变化
//     决定是否重置）；返回前不等待 LLM；调用 `try_start_backfill` 触发后台
//     AI 增强（auto_enrich 关闭或无 config 时是 no-op）。
//   * `refresh_ai_metadata` 把 metadata 重置为 pending 然后触发 backfill；
//     命令本身立即返回。
//   * `remove_ai_tag` 只删 source='ai' 行，manual 永远保留。
//   * `update_manual_tags` 只写 manual 行，AI 行不动。
//   * `ai_backfill_status` 返回当前 DB 中各状态的 entry 计数。

use tauri::{AppHandle, State};

use crate::vault::ipc::VaultRuntimeState;
use crate::vault::models::{
    BackfillStatus, EntryKind, VaultEntryDetail, VaultEntryInput, VaultEntrySummary,
};
use crate::vault::storage as vstore;

/// 创建条目（pending metadata）+ 触发 backfill。命令本身不等待 LLM。
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
    // 触发后台 backfill（auto_enrich 关闭或无 config → 内部 no-op）
    crate::vault::jobs::try_start_backfill(&app);
    Ok(detail)
}

/// 更新条目。若内容 hash 变化，metadata 自动重置为 pending。
#[tauri::command]
pub async fn ipc_vault_update_entry(
    state: State<'_, crate::AppState>,
    app: AppHandle,
    id: String,
    input: VaultEntryInput,
) -> Result<VaultEntryDetail, String> {
    let detail = {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        vstore::update_entry(&mut conn, &id, &input).map_err(|e| e.to_string())?
    };
    // 内容可能变了 → 触发 backfill
    crate::vault::jobs::try_start_backfill(&app);
    Ok(detail)
}

/// 删除条目（含 fields / tags / metadata / FTS，由 FK + 显式 SQL 处理）。
#[tauri::command]
pub async fn ipc_vault_delete_entry(
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<(), String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    vstore::delete_entry(&mut conn, &id).map_err(|e| e.to_string())?;
    Ok(())
}

/// 列出条目（可按 kind 过滤）。
#[tauri::command]
pub async fn ipc_vault_list_entries(
    state: State<'_, crate::AppState>,
    kind: Option<EntryKind>,
) -> Result<Vec<VaultEntrySummary>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    vstore::list_entries(&conn, kind).map_err(|e| e.to_string())
}

/// 获取条目详情（含 fields / tags / ai_metadata）。
#[tauri::command]
pub async fn ipc_vault_get_entry(
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<VaultEntryDetail, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    vstore::get_entry_detail(&conn, &id).map_err(|e| e.to_string())
}

/// 覆盖式写入 manual tags（AI tags 不动）。
#[tauri::command]
pub async fn ipc_vault_update_manual_tags(
    state: State<'_, crate::AppState>,
    id: String,
    tags: Vec<String>,
) -> Result<VaultEntryDetail, String> {
    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        vstore::set_manual_tags(&mut conn, &id, &tags).map_err(|e| e.to_string())?;
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    vstore::get_entry_detail(&conn, &id).map_err(|e| e.to_string())
}

/// 删除指定的 AI tag（normalized_tag 匹配；manual 行不动）。
#[tauri::command]
pub async fn ipc_vault_remove_ai_tag(
    state: State<'_, crate::AppState>,
    id: String,
    normalized_tag: String,
) -> Result<VaultEntryDetail, String> {
    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        vstore::remove_ai_tag(&mut conn, &id, &normalized_tag).map_err(|e| e.to_string())?;
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    vstore::get_entry_detail(&conn, &id).map_err(|e| e.to_string())
}

/// 重新触发某条目的 AI metadata 增强。
///
/// 命令立即返回；实际工作在后台 backfill worker 中进行。
/// 实现策略：把 metadata 重置为 pending（保留 content_hash 不变），然后
/// 触发 backfill；worker 自然会拉取并处理。
#[tauri::command]
pub async fn ipc_vault_refresh_ai_metadata(
    state: State<'_, crate::AppState>,
    vault: State<'_, VaultRuntimeState>,
    app: AppHandle,
    id: String,
) -> Result<(), String> {
    // 若无 config 或 auto_enrich 关闭，refresh 仍然重置 pending（让用户看到
    // 状态变化），但 backfill 不会真的跑。
    let content_hash = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        vstore::ai_content_hash_for_entry(&conn, &id).map_err(|e| e.to_string())?
    };
    // 即便 hash 为空（entry 没记录）也允许继续；mark_pending 会创建一条新行。
    {
        let mut conn = state.db.lock().map_err(|e| e.to_string())?;
        vstore::mark_ai_metadata_pending(&mut conn, &id, &content_hash)
            .map_err(|e| e.to_string())?;
    }
    // 让 worker 跳过门控检查（用户主动 refresh）—— 通过短暂清除 cooldown
    // 不是我们这里要做的事；worker 内部会自行检查 should_skip_automatic_call。
    // 这里只触发 backfill。
    let _ = vault; // 标记参数被显式接受（保留扩展点）
    crate::vault::jobs::try_start_backfill(&app);
    Ok(())
}

/// 返回当前 AI metadata 各状态计数。
#[tauri::command]
pub async fn ipc_vault_ai_backfill_status(
    state: State<'_, crate::AppState>,
) -> Result<BackfillStatus, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    vstore::backfill_status(&conn).map_err(|e| e.to_string())
}

// ---- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratchpad::storage::ensure_dock_schema;
    use crate::vault::models::{EntryKind, FieldInput, VaultEntryInput};
    use rusqlite::Connection;

    fn open_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn, 0).unwrap();
        vstore::ensure_vault_schema(&mut conn).unwrap();
        conn
    }

    fn sample_input() -> VaultEntryInput {
        VaultEntryInput {
            kind: EntryKind::Note,
            title: "hello".into(),
            fields: vec![FieldInput {
                key: "user".into(),
                value: "admin".into(),
                is_sensitive: false,
            }],
            notes: None,
            manual_tags: vec![],
        }
    }

    #[test]
    fn create_entry_writes_pending_metadata_and_returns_detail() {
        let mut conn = open_db();
        let detail = vstore::create_entry(&mut conn, &sample_input()).unwrap();
        let md = vstore::get_ai_metadata(&conn, &detail.entry.id)
            .unwrap()
            .unwrap();
        assert_eq!(md.status, crate::vault::models::AiMetadataStatus::Pending);
        assert!(!detail.fields.is_empty());
    }

    #[test]
    fn update_entry_changes_title_resets_metadata_to_pending() {
        let mut conn = open_db();
        let d = vstore::create_entry(&mut conn, &sample_input()).unwrap();
        let id = d.entry.id.clone();
        // 模拟 ready 状态
        vstore::set_ai_metadata(
            &mut conn,
            &crate::vault::models::VaultAiMetadata {
                entry_id: id.clone(),
                summary: Some("s".into()),
                search_aliases: vec![],
                content_hash: "old".into(),
                provider_id: None,
                model: None,
                generated_at: None,
                status: crate::vault::models::AiMetadataStatus::Ready,
            },
        )
        .unwrap();
        let mut input2 = sample_input();
        input2.title = "changed".into();
        vstore::update_entry(&mut conn, &id, &input2).unwrap();
        let md = vstore::get_ai_metadata(&conn, &id).unwrap().unwrap();
        assert_eq!(md.status, crate::vault::models::AiMetadataStatus::Pending);
    }

    #[test]
    fn remove_ai_tag_only_deletes_ai_source() {
        let mut conn = open_db();
        let d = vstore::create_entry(&mut conn, &sample_input()).unwrap();
        let id = d.entry.id.clone();
        vstore::replace_ai_tags(&mut conn, &id, &["ai-only".into()]).unwrap();
        vstore::set_manual_tags(&mut conn, &id, &["manual-only".into()]).unwrap();
        // 删 ai tag
        vstore::remove_ai_tag(&mut conn, &id, "ai-only").unwrap();
        let tags = vstore::list_tags_with_source(&conn, &id).unwrap();
        assert!(!tags.iter().any(|t| t.tag == "ai-only"));
        assert!(tags.iter().any(|t| t.tag == "manual-only"));
    }

    #[test]
    fn backfill_status_counts_zero_on_empty_db() {
        let conn = open_db();
        let s = vstore::backfill_status(&conn).unwrap();
        assert_eq!(s.total, 0);
        assert_eq!(s.ready, 0);
        assert_eq!(s.pending, 0);
        assert_eq!(s.error, 0);
    }
}
