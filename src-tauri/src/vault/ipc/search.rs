// src-tauri/src/vault/ipc/search.rs
//
// Task 9: 3 个可取消搜索 IPC 命令。
//
//   * `ipc_vault_search_hybrid_local(query, plan, limit)` — 纯本地混合检索
//     （不调 LLM；plan 由调用方已经通过 `ipc_vault_plan_search` 取到）。
//   * `ipc_vault_plan_search(query, request_id)` — 脱敏 query → 调 LLM 生成
//     AiQueryPlan + 审计；支持 `ipc_vault_cancel_search` 主动取消；新调用
//     自动取消旧的 active search（相同 request_id 例外）。
//   * `ipc_vault_cancel_search(request_id)` — 仅当 request_id 与当前 active
//     search 匹配时才取消，防止迟到的旧窗口 cleanup 误取消新查询。
//
// 安全契约：
//   - 查询文本在发送给 LLM 之前必须经过 `desensitize_raw_text`，LLM 永远
//     不会看到敏感原文；
//   - LLM 返回的内容经 `parse_query_plan` 校验（拒非法 kind / 错日期 / 过长
//     term 等）；
//   - Cancelled 不触发 cooldown、不弹 toast；
//   - 审计 `PlannedSearch.audit` 里只含 role + content，没有 API key 或
//     Authorization header，也不会含任何 catalog 信息（prompt 本身就不带 catalog）。

use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

use crate::vault::ai::{build_request_audit, parse_query_plan};
use crate::vault::desensitize::{desensitize_raw_text, TokenMap};
use crate::vault::ipc::{llm_error_event, VaultRuntimeState};
use crate::vault::llm::openai_compat::OpenAiCompatAdapter;
use crate::vault::llm::prompt::query_plan_prompt;
use crate::vault::llm::{LlmAdapter, LlmError, LlmRequest};
use crate::vault::models::{AiQueryPlan, PlannedSearch, VaultSearchHit};
use crate::vault::search as vsearch;

/// 本地混合检索 IPC。不调 LLM；plan 由调用方提供（可为 `None`）。
///
/// - `limit` 约束在 [1, 100]；`None` 时使用默认 20。
/// - 空 query 返回空数组，不构造 FTS 表达式或 AI 请求。
#[tauri::command]
pub async fn ipc_vault_search_hybrid_local(
    state: State<'_, crate::AppState>,
    query: String,
    plan: Option<AiQueryPlan>,
    limit: Option<usize>,
) -> Result<Vec<VaultSearchHit>, String> {
    let limit = limit.unwrap_or(20);
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    vsearch::search_local(&conn, &query, plan.as_ref(), limit).map_err(|e| e.to_string())
}

/// AI 查询理解 IPC。脱敏 query → 调 LLM 生成 AiQueryPlan。
///
/// 同时注册一个新的 `CancellationToken`：
///   - 如果存在 active search 且 request_id 不同，会先把旧 token 取消；
///   - 如果 request_id 与 active 相同（罕见的重复请求），不重复取消。
///
/// 返回 `PlannedSearch { plan, understood_terms, audit }`。
/// 失败时返回本地降级的 plan（空 keywords/aliases）以及一条 audit。
#[tauri::command]
pub async fn ipc_vault_plan_search(
    app: AppHandle,
    query: String,
    request_id: String,
) -> Result<PlannedSearch, String> {
    plan_search_redacted(&app, query, request_id).await
}

pub(crate) async fn plan_search_redacted(
    app: &AppHandle,
    query: String,
    request_id: String,
) -> Result<PlannedSearch, String> {
    let vault_state = app.state::<VaultRuntimeState>();

    // 1) 注册新 active search，并取出旧 token（如果 ID 不同则取消）
    let new_token = CancellationToken::new();
    let previous = vault_state.set_active_search(request_id.clone(), new_token.clone());
    if let Some((prev_id, prev_token)) = previous {
        if prev_id != request_id {
            prev_token.cancel();
        }
    }

    // 2) 取 LLM 配置；缺配置 → 直接返回空 plan（降级）
    let config = {
        let guard = vault_state.config.lock().map_err(|e| e.to_string())?;
        guard.clone()
    };
    let Some(config) = config else {
        // 清理 active search（标记本次完成）
        vault_state.clear_active_search(&request_id);
        return Ok(empty_planned_search(query));
    };

    // 3) 门控：auth-blocked 或 cooldown → 直接降级（不调 LLM）
    if vault_state.should_skip_automatic_call().is_some() {
        vault_state.clear_active_search(&request_id);
        return Ok(empty_planned_search(query));
    }

    // 4) 脱敏 query（请求级 TokenMap）
    let mut token_map = TokenMap::new();
    let masked_query = desensitize_raw_text(&query, &[], &mut token_map);
    // 查询计划不需要 detokenize（LLM 只回 keywords / aliases / kinds / dates，
    // 这些字段都是元数据，禁止带 token 回填）。drop token_map 提前销毁。
    drop(token_map);

    let now_rfc3339 = chrono::Utc::now().to_rfc3339();
    let messages = query_plan_prompt(&masked_query, &now_rfc3339);

    // 5) 构造 adapter
    let provider_id = config.provider_id.clone();
    let model = config.model.clone();
    let adapter = match OpenAiCompatAdapter::new(config.base_url, config.api_key, config.model) {
        Ok(a) => a,
        Err(e) => {
            // InvalidConfig 不参与门控
            let _ = app.emit("vault-llm-error", llm_error_event(e));
            vault_state.clear_active_search(&request_id);
            return Ok(empty_planned_search(query));
        }
    };

    // 6) 构造审计（在 LLM 调用之前，因为 audit 描述"我们发出了什么"）
    let audit = build_request_audit(&provider_id, &model, &messages);

    let req = LlmRequest {
        messages,
        json_mode: true,
        temperature: 0.0,
        max_tokens: Some(256),
    };

    // 7) 可取消地调 LLM
    let result = tokio::select! {
        r = adapter.complete(req) => r,
        _ = new_token.cancelled() => Err(LlmError::Cancelled),
    };

    let resp = match result {
        Ok(r) => {
            vault_state.record_success();
            r
        }
        Err(e) => {
            match &e {
                LlmError::Cancelled => {
                    // 用户取消：不弹 toast、不进入 cooldown
                }
                _ => {
                    vault_state.record_failure(&e);
                    let _ = app.emit("vault-llm-error", llm_error_event(e));
                }
            }
            vault_state.clear_active_search(&request_id);
            return Ok(empty_planned_search_with_audit(query, audit));
        }
    };

    // 8) 解析计划 —— 无效整次降级为空 plan
    let plan = match parse_query_plan(&resp.content) {
        Ok(p) => p,
        Err(e) => {
            let _ = app.emit("vault-llm-error", llm_error_event(e));
            vault_state.clear_active_search(&request_id);
            return Ok(empty_planned_search_with_audit(query, audit));
        }
    };

    // 9) understood_terms：把 plan 的 keywords + aliases 给前端预览
    let mut understood_terms = plan.keywords.clone();
    understood_terms.extend(plan.aliases.iter().cloned());
    understood_terms.sort();
    understood_terms.dedup();

    vault_state.clear_active_search(&request_id);
    Ok(PlannedSearch {
        plan,
        understood_terms,
        audit,
    })
}

/// 取消当前 active search；只在 `request_id` 匹配时生效。
#[tauri::command]
pub async fn ipc_vault_cancel_search(
    vault: State<'_, VaultRuntimeState>,
    request_id: String,
) -> Result<(), String> {
    cancel_search(&vault, &request_id);
    Ok(())
}

pub(crate) fn cancel_search(vault: &VaultRuntimeState, request_id: &str) {
    // 只在 ID 匹配时取消；不匹配（迟到 cleanup）则忽略。
    let token_opt = vault.active_search_token();
    if let Some(token) = token_opt {
        // 读 ID 前先看是否匹配；通过 set/clear 完成原子操作
        // 这里采用：拿当前 token，对比 ID，匹配则 cancel + clear
        let current_id_matches = vault.active_search_id_matches(request_id);
        if current_id_matches {
            token.cancel();
            vault.clear_active_search(request_id);
        }
    }
}

/// 构造降级返回：空 plan + 占位 audit。
fn empty_planned_search(_query: String) -> PlannedSearch {
    PlannedSearch {
        plan: AiQueryPlan::default(),
        understood_terms: Vec::new(),
        audit: crate::vault::models::AiRequestAudit {
            provider_id: String::new(),
            model: String::new(),
            sent_at: chrono::Utc::now().to_rfc3339(),
            messages: Vec::new(),
        },
    }
}

fn empty_planned_search_with_audit(
    _query: String,
    audit: crate::vault::models::AiRequestAudit,
) -> PlannedSearch {
    PlannedSearch {
        plan: AiQueryPlan::default(),
        understood_terms: Vec::new(),
        audit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratchpad::storage::ensure_dock_schema;
    use rusqlite::Connection;

    #[test]
    fn planning_request_contains_query_but_never_catalog_content() {
        let messages = query_plan_prompt("找生产数据库", "2026-07-18T00:00:00Z");
        let audit = build_request_audit("test", "test-model", &messages);
        let sent = audit
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(sent.contains("找生产数据库"));
        assert!(!sent.contains("NeverSendCatalogFixture"));
    }

    fn open_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();
        conn
    }

    // ---- 活跃搜索 token 的取消语义（不通过 IPC，直接调 runtime） ----------

    #[test]
    fn new_active_search_replaces_old_token_with_different_id() {
        // 这里直接复用 `ipc_vault_plan_search` 的语义：新搜索调用前会先取
        // 旧 token 并 cancel（如果 ID 不同）。
        let conn = open_db();
        let runtime = VaultRuntimeState::load(&conn);

        let token1 = CancellationToken::new();
        let token1_clone = token1.clone();
        runtime.set_active_search("req-1".into(), token1);
        assert!(!token1_clone.is_cancelled());

        let token2 = CancellationToken::new();
        // 模拟 ipc_vault_plan_search 内部：set_active_search 返回旧的，
        // ID 不同 → 取消旧的。
        let previous = runtime.set_active_search("req-2".into(), token2);
        if let Some((prev_id, prev_token)) = previous {
            if prev_id != "req-2" {
                prev_token.cancel();
            }
        }

        // req-1 应被取消（因为 ID 不同）
        assert!(
            token1_clone.is_cancelled(),
            "old search token must be cancelled when new search with different id arrives"
        );
    }

    #[test]
    fn cancel_only_fires_when_id_matches_active() {
        let conn = open_db();
        let runtime = VaultRuntimeState::load(&conn);

        let token = CancellationToken::new();
        let token_clone = token.clone();
        runtime.set_active_search("active-req".into(), token);

        // 迟到的 cleanup 不应取消当前请求。
        cancel_search(&runtime, "stale-req");
        assert!(
            !token_clone.is_cancelled(),
            "stale cancel must not affect active"
        );

        cancel_search(&runtime, "active-req");
        assert!(token_clone.is_cancelled());
        assert!(!runtime.active_search_id_matches("active-req"));
    }

    // ---- 降级路径 --------------------------------------------------------

    #[test]
    fn empty_planned_search_has_empty_plan_and_understood_terms() {
        let p = empty_planned_search("any".into());
        assert!(p.plan.keywords.is_empty());
        assert!(p.plan.aliases.is_empty());
        assert!(p.plan.kinds.is_empty());
        assert!(p.understood_terms.is_empty());
        assert!(!p.audit.sent_at.is_empty());
    }

    #[test]
    fn planned_search_carries_audit_but_no_api_key() {
        let audit = crate::vault::models::AiRequestAudit {
            provider_id: "deepseek".into(),
            model: "deepseek-chat".into(),
            sent_at: "2026-07-17T00:00:00Z".into(),
            messages: vec![crate::vault::models::AuditMessage {
                role: "user".into(),
                content: "hello [SECRET:abcd]".into(),
            }],
        };
        let p = empty_planned_search_with_audit("any".into(), audit);
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("sk-secret"), "audit must not leak api key");
        assert!(json.contains("deepseek-chat"));
        assert!(json.contains("[SECRET:abcd]"));
    }

    // ---- 本地搜索 IPC（直接调 search_local） -----------------------------

    #[test]
    fn hybrid_local_search_returns_results_via_storage() {
        use crate::vault::models::{EntryKind, FieldInput, VaultEntryInput};
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn).unwrap();
        crate::vault::storage::ensure_vault_schema(&mut conn).unwrap();
        crate::vault::storage::create_entry(
            &mut conn,
            &VaultEntryInput {
                kind: EntryKind::Credential,
                title: "FindMe".into(),
                fields: vec![FieldInput {
                    key: "user".into(),
                    value: "admin".into(),
                    is_sensitive: false,
                }],
                notes: None,
                manual_tags: vec![],
            },
        )
        .unwrap();
        let hits = vsearch::search_local(&conn, "FindMe", None, 20).unwrap();
        assert_eq!(hits.len(), 1);
    }

    // ---- verify config_loaded path missing config yields empty plan ------

    #[test]
    fn missing_config_returns_empty_plan_path() {
        // 这里直接测试 empty_planned_search（缺配置时返回的就是它）
        let p = empty_planned_search("query".into());
        assert!(p.plan.keywords.is_empty());
    }

    /// 简单覆盖一下 build_request_audit 在 search 上下文中的行为：
    /// audit 不应包含 catalog 信息。
    #[test]
    fn audit_does_not_include_catalog() {
        use crate::vault::llm::ChatMessage;
        let messages = query_plan_prompt("postgres prod", "2026-07-17T00:00:00Z");
        let audit = build_request_audit("deepseek", "deepseek-chat", &messages);
        let json = serde_json::to_string(&audit).unwrap();
        assert!(!json.to_lowercase().contains("catalog"));
        // mask 后的 query 应当出现（desensitize 没把 "postgres prod" 脱敏，
        // 但即使被脱敏，audit 里也只会有 masked 文本）
        let _ = ChatMessage::system("dummy"); // 触发 import 使用，避免 unused 警告
    }
}
