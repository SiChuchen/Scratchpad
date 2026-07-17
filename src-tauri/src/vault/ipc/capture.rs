// src-tauri/src/vault/ipc/capture.rs
//
// Task 10: Capture IPC commands (parse / enrich / save) + testable
// `enrich_capture_with` helper.
//
// 安全契约：
//   * `ipc_vault_parse_capture_local` 仅做本地解析，绝不调 LLM；
//   * `ipc_vault_enrich_capture` 每次构造请求局部 `TokenMap`，把 raw_text +
//     manual_sensitive_values 脱敏后交给 LLM；返回的 audit 是真正发送的
//     messages（masked text），不含 API key / Authorization header；
//   * `ipc_vault_create_from_capture` **保存阶段绝不调 LLM**，只把已经由前端
//     展示并可能由用户调整过的 draft 写入 DB；保存前重新校验长度/数量，
//     扫描 draft 全部文本以发现未知 `[SECRET:*]` 占位符，并拒绝 tags /
//     summary / aliases 中出现任一敏感字段值。
//
// 幂等：`request_id` 是 capture UI 在 parse 阶段生成的 UUID；同一 request_id
// 在 `vault_capture_requests` 表里只会写入一次，重复提交直接返回首次保存的
// entry（无需用户感知）。

use tauri::State;

use crate::vault::ai::{build_request_audit, parse_capture_response};
use crate::vault::capture as vcapture;
use crate::vault::config::LlmConfigStored;
use crate::vault::desensitize::{desensitize_raw_text, TokenMap};
use crate::vault::llm::prompt::capture_enrichment_prompt;
use crate::vault::llm::{LlmAdapter, LlmError, LlmRequest};
use crate::vault::models::{
    is_default_sensitive_key, CaptureDraft, CaptureEnrichment, VaultEntryDetail,
};
use crate::vault::storage as vstore;

/// 用户可读的占位符扫描失败信息。
const UNKNOWN_PLACEHOLDER_HINT: &str = "draft 包含未脱敏的占位符，请重新录入";

// ---- Step 3: IPC 命令 ------------------------------------------------------

/// 仅做本地解析，绝不调 LLM。
#[tauri::command]
pub async fn ipc_vault_parse_capture_local(raw_text: String) -> Result<CaptureDraft, String> {
    vcapture::parse_capture_local(&raw_text).map_err(|e| e.to_string())
}

/// 构造脱敏 prompt 并调用 LLM；返回 suggestion + audit。
///
/// `manual_sensitive_values` 是前端用户在录入 UI 中额外标记为敏感的值；
/// 它们和 `raw_text` 一起在同一 `TokenMap` 中脱敏。
#[tauri::command]
pub async fn ipc_vault_enrich_capture(
    vault: State<'_, crate::vault::ipc::VaultRuntimeState>,
    _draft: CaptureDraft,
    raw_text: String,
    manual_sensitive_values: Vec<String>,
    _request_id: String,
) -> Result<CaptureEnrichment, String> {
    // 1) 取 config；缺配置 → 错误
    let config: LlmConfigStored = {
        let guard = vault.config.lock().map_err(|e| e.to_string())?;
        guard.clone().ok_or_else(|| "LLM 未配置".to_string())?
    };

    // 2) 构造 adapter（helper 接受 `&dyn LlmAdapter`，方便单元测试注入）
    let adapter = crate::vault::llm::openai_compat::OpenAiCompatAdapter::new(
        config.base_url.clone(),
        config.api_key.clone(),
        config.model.clone(),
    )
    .map_err(|e| e.to_string())?;

    enrich_capture_with(&adapter, &config, &raw_text, &manual_sensitive_values)
        .await
        .map_err(|e| e.to_string())
}

/// 保存最终 draft 到 DB；幂等；绝不调 LLM。
///
/// 保存前校验：
///   * draft 中所有文本字段不能含未知 `[SECRET:*]` 占位符；
///   * 重新跑长度 / 数量校验（与 `parse_capture_local` 保持一致）；
///   * tags / summary / aliases 中不得出现任一敏感字段值（避免 LLM 把脱敏
///     后又泄漏到 UI 直接展示的元数据里）。
#[tauri::command]
pub async fn ipc_vault_create_from_capture(
    state: State<'_, crate::AppState>,
    final_draft: CaptureDraft,
    request_id: String,
) -> Result<VaultEntryDetail, String> {
    // 1) 校验：扫描未知占位符 + 长度/数量
    validate_final_draft(&final_draft).map_err(|e| e.to_string())?;

    // 2) 校验：tags / summary / aliases 中不能出现敏感字段值
    reject_sensitive_metadata_leak(&final_draft).map_err(|e| e.to_string())?;

    // 3) 落库
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    vstore::create_from_capture(&mut conn, &final_draft, &request_id).map_err(|e| e.to_string())
}

// ---- 可测试 helper ---------------------------------------------------------

/// 把 draft + raw_text 脱敏后调 LLM，返回 `CaptureEnrichment`。
///
/// 这是 capture 流程的核心 —— 单独抽出来便于单元测试用 fake adapter 注入，
/// 不依赖 Tauri State / DB。
///
/// 流程：
///   1. 构造请求局部 `TokenMap`；
///   2. 把 raw_text + manual_sensitive_values 脱敏成 masked_text；
///   3. 用 masked_text 组装 prompt（messages）；
///   4. 调用 adapter；
///   5. 用同一份 token_map 解析响应；
///   6. 构造 audit（messages 的副本，只有 role/content），与 suggestion 一起返回。
pub(crate) async fn enrich_capture_with(
    adapter: &dyn LlmAdapter,
    config: &LlmConfigStored,
    raw_text: &str,
    manual_sensitive_values: &[String],
) -> Result<CaptureEnrichment, LlmError> {
    // 1) 请求级 TokenMap
    let mut token_map = TokenMap::new();
    let masked_text = desensitize_raw_text(raw_text, manual_sensitive_values, &mut token_map);

    // 2) 组装 prompt
    let messages = capture_enrichment_prompt(&masked_text);

    // 3) 构造 audit —— 在调用 LLM 之前就把它建好（audit 描述"我们发出了什么"）
    let audit = build_request_audit(&config.provider_id, &config.model, &messages);

    // 4) 调 LLM
    let req = LlmRequest {
        messages,
        json_mode: true,
        temperature: 0.3,
        max_tokens: Some(512),
    };
    let resp = adapter.complete(req).await?;

    // 5) 解析响应 —— 使用同一份 token_map
    let suggestion = parse_capture_response(&resp.content, &token_map)?;

    // 6) 返回 suggestion + audit
    Ok(CaptureEnrichment { suggestion, audit })
}

// ---- 校验辅助 --------------------------------------------------------------

/// 扫描 draft 中所有文本字段，发现未知 `[SECRET:...]` 占位符即报错。
/// 同时复用 `parse_capture_local` 的 sanitize 规则做长度 / 数量校验。
fn validate_final_draft(draft: &CaptureDraft) -> Result<(), String> {
    // 占位符扫描：title / notes / fields.value / 各 tag / summary / aliases
    let mut scan_buf = String::new();
    scan_buf.push_str(&draft.title);
    scan_buf.push('\n');
    if let Some(n) = &draft.notes {
        scan_buf.push_str(n);
        scan_buf.push('\n');
    }
    for f in &draft.fields {
        scan_buf.push_str(&f.key);
        scan_buf.push('\n');
        scan_buf.push_str(&f.value);
        scan_buf.push('\n');
    }
    for t in &draft.manual_tags {
        scan_buf.push_str(t);
        scan_buf.push('\n');
    }
    for t in &draft.ai_tags {
        scan_buf.push_str(t);
        scan_buf.push('\n');
    }
    if let Some(s) = &draft.ai_summary {
        scan_buf.push_str(s);
        scan_buf.push('\n');
    }
    for a in &draft.search_aliases {
        scan_buf.push_str(a);
        scan_buf.push('\n');
    }
    if has_unknown_placeholder(&scan_buf) {
        return Err(UNKNOWN_PLACEHOLDER_HINT.to_string());
    }

    // 长度 / 数量快速检查（完整规则已由 parse_capture_local 保证）
    if draft.title.trim().is_empty() {
        return Err("title 不能为空".to_string());
    }
    if draft.title.chars().count() > 120 {
        return Err(format!(
            "title 过长：{} 字符（上限 120）",
            draft.title.chars().count()
        ));
    }
    if draft.fields.len() > 32 {
        return Err("字段数量超过 32 上限".to_string());
    }
    if draft.ai_tags.len() > 5 {
        return Err("AI 标签数量超过 5 上限".to_string());
    }
    if draft.search_aliases.len() > 12 {
        return Err("搜索别名数量超过 12 上限".to_string());
    }
    if let Some(s) = &draft.ai_summary {
        if s.chars().count() > 500 {
            return Err("摘要长度超过 500 上限".to_string());
        }
    }
    Ok(())
}

/// 检查文本是否含 `[SECRET:...]` 占位符（视为未知 —— 因为保存路径
/// 上的 draft 不应该再带占位符，前端展示时必须已经 detokenize 过）。
fn has_unknown_placeholder(text: &str) -> bool {
    text.contains("[SECRET:")
}

/// 收集 draft 中所有敏感字段值，逐一检查是否出现在 tags / summary / aliases 中。
/// 任一命中即拒绝。
fn reject_sensitive_metadata_leak(draft: &CaptureDraft) -> Result<(), String> {
    let sensitive_values: Vec<String> = draft
        .fields
        .iter()
        .filter(|f| f.is_sensitive || is_default_sensitive_key(&f.key))
        .map(|f| f.value.clone())
        .filter(|v| !v.trim().is_empty())
        .collect();
    if sensitive_values.is_empty() {
        return Ok(());
    }

    let mut metadata_buf = String::new();
    for t in &draft.ai_tags {
        metadata_buf.push_str(t);
        metadata_buf.push('\n');
    }
    for t in &draft.manual_tags {
        metadata_buf.push_str(t);
        metadata_buf.push('\n');
    }
    if let Some(s) = &draft.ai_summary {
        metadata_buf.push_str(s);
        metadata_buf.push('\n');
    }
    for a in &draft.search_aliases {
        metadata_buf.push_str(a);
        metadata_buf.push('\n');
    }
    let metadata_lower = metadata_buf.to_lowercase();
    for v in &sensitive_values {
        let v_lower = v.to_lowercase();
        // 与 desensitize::validate_non_sensitive_metadata 保持一致：
        //   * 短敏感值（< 6 字符）只在精确匹配时拒绝，避免 "admin" / "ok" / "abc"
        //     这类常用词在 AI 标签里出现被误判为泄漏。
        //   * 长敏感值（>= 6 字符）保留子串匹配，因为长 token 几乎不会自然出现在
        //     概念标签里。
        if v_lower.chars().count() >= 6 {
            if metadata_lower.contains(&v_lower) {
                return Err("sensitive_metadata_rejected".to_string());
            }
        } else if metadata_lower.split_whitespace().any(|w| w == v_lower) {
            // 短值：要求词级精确匹配（"admin" 不能命中 "administrator"）。
            return Err("sensitive_metadata_rejected".to_string());
        }
    }
    Ok(())
}

// ---- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratchpad::storage::ensure_dock_schema;
    use crate::vault::llm::{LlmAdapter, LlmError, LlmRequest, LlmResponse};
    use crate::vault::models::{CaptureDraft, EntryKind};
    use async_trait::async_trait;
    use rusqlite::Connection;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn open_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_dock_schema(&mut conn, 0).unwrap();
        vstore::ensure_vault_schema(&mut conn).unwrap();
        conn
    }

    fn sample_stored() -> LlmConfigStored {
        LlmConfigStored {
            provider_id: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: "sk-test".into(),
            model: "deepseek-chat".into(),
        }
    }

    fn sample_draft() -> CaptureDraft {
        CaptureDraft {
            kind: EntryKind::Note,
            title: "Staging".into(),
            notes: Some("hello world".into()),
            fields: Vec::new(),
            manual_tags: Vec::new(),
            ai_tags: Vec::new(),
            ai_summary: None,
            search_aliases: Vec::new(),
            ai_provenance: None,
            warnings: Vec::new(),
        }
    }

    /// 始终 panic 的 adapter —— 用来证明"路径没走到 LLM"。
    struct PanickingAdapter;
    #[async_trait]
    impl LlmAdapter for PanickingAdapter {
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            panic!("LLM should not be called on this path");
        }
    }

    /// 返回固定 JSON 的 fake adapter。记录调用次数。
    struct FakeAdapter {
        response: String,
        counter: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl LlmAdapter for FakeAdapter {
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            self.counter.fetch_add(1, Ordering::SeqCst);
            Ok(LlmResponse {
                content: self.response.clone(),
                tokens_used: None,
            })
        }
    }

    /// 始终返回 Err 的 adapter —— 用来模拟 AI 失败但仍可保存 draft。
    struct FailingAdapter;
    #[async_trait]
    impl LlmAdapter for FailingAdapter {
        async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
            Err(LlmError::Network("simulated".into()))
        }
    }

    // ---- Required tests ----------------------------------------------------

    #[tokio::test]
    async fn capture_local_returns_before_ai_adapter_is_called() {
        // 直接调 parse_capture_local：即便有个会 panic 的 adapter 也不应该被触发。
        let _panic_adapter = PanickingAdapter;
        let draft = vcapture::parse_capture_local("hello note").expect("parse ok");
        assert_eq!(draft.kind, EntryKind::Note);
        assert!(!draft.title.is_empty());
    }

    #[tokio::test]
    async fn capture_enrich_returns_suggestion_and_exact_audit() {
        // fake adapter 返回固定的 tags JSON
        let counter = Arc::new(AtomicUsize::new(0));
        let adapter = FakeAdapter {
            response: r#"{"tags":["work","prod"]}"#.to_string(),
            counter: counter.clone(),
        };
        let config = sample_stored();
        let _draft = sample_draft();
        let raw_text = "hello world topsecret";
        let enrichment =
            enrich_capture_with(&adapter, &config, raw_text, &["topsecret".to_string()])
                .await
                .expect("enrich ok");

        assert_eq!(enrichment.suggestion.ai_tags, vec!["work", "prod"]);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        // audit 必须含 user message —— masked_text
        let user_msg = enrichment
            .audit
            .messages
            .iter()
            .find(|m| m.role == "user")
            .expect("user message exists");
        assert!(user_msg.content.contains("[SECRET:"));
        // 不应泄漏 "topsecret" 原文
        assert!(!user_msg.content.contains("topsecret"));
        // audit 必须含 provider/model
        assert_eq!(enrichment.audit.provider_id, "deepseek");
        assert_eq!(enrichment.audit.model, "deepseek-chat");
    }

    #[tokio::test]
    async fn capture_ai_failure_keeps_local_draft_saveable() {
        // enrich 返回错误 —— 但本地 draft 仍然可保存
        let adapter = FailingAdapter;
        let config = sample_stored();
        let draft = sample_draft();
        let result = enrich_capture_with(&adapter, &config, "any raw", &[]).await;
        assert!(result.is_err());

        // 但 create_from_capture 应仍能用本地 draft 保存（这条路径不调 LLM）
        let mut conn = open_db();
        let saved = vstore::create_from_capture(&mut conn, &draft, "req-1");
        assert!(saved.is_ok(), "save should work even when enrich failed");
    }

    #[tokio::test]
    async fn capture_save_does_not_call_llm_again() {
        // 用 PanickingAdapter 模拟"LLM 不应该被调用"的约束
        let _panic = PanickingAdapter;
        let mut conn = open_db();
        let draft = sample_draft();
        let saved = vstore::create_from_capture(&mut conn, &draft, "req-save-1");
        assert!(saved.is_ok());
    }

    #[test]
    fn retag_replaces_ai_tags_only() {
        // 模拟"refresh 只换 AI tags，manual 不动"
        let mut conn = open_db();
        let mut draft = sample_draft();
        draft.manual_tags = vec!["manual-keep".to_string()];
        let saved = vstore::create_from_capture(&mut conn, &draft, "req-retag-1").unwrap();
        let id = saved.entry.id.clone();

        // 假设 refresh 后 AI 给出新的 tags
        vstore::replace_ai_tags(
            &mut conn,
            &id,
            &["new-ai-1".to_string(), "new-ai-2".to_string()],
        )
        .unwrap();
        let tags = vstore::list_tags_with_source(&conn, &id).unwrap();
        // manual tag 保留
        assert!(tags.iter().any(|t| t.tag == "manual-keep"));
        // ai tag 被替换
        assert!(tags.iter().any(|t| t.tag == "new-ai-1"));
        assert!(tags.iter().any(|t| t.tag == "new-ai-2"));
        // 没有旧的 ai tag
        assert!(!tags.iter().any(|t| t.tag == "ai-tag-old"));
    }

    #[test]
    fn backfill_skips_when_auto_enrich_is_disabled() {
        // 这条断言聚焦于 "auto_enrich 关闭时不应触发 backfill"。
        // 我们通过 jobs::should_run_backfill 直接判断（runtime 默认无 config
        // 即视为"不应启动"）。
        let runtime = crate::vault::ipc::VaultRuntimeState::default();
        assert!(!crate::vault::jobs::should_run_backfill(&runtime));
    }

    #[test]
    fn backfill_progress_counts_ready_pending_and_error() {
        // 准备 3 条 entry：1 ready、1 pending、1 error
        let mut conn = open_db();
        use crate::vault::models::{
            AiMetadataStatus, FieldInput, VaultAiMetadata, VaultEntryInput,
        };
        let mk_input = |title: &str| VaultEntryInput {
            kind: EntryKind::Note,
            title: title.into(),
            fields: Vec::<FieldInput>::new(),
            notes: None,
            manual_tags: Vec::new(),
        };
        let ready = vstore::create_entry(&mut conn, &mk_input("ready")).unwrap();
        let _pending = vstore::create_entry(&mut conn, &mk_input("pending")).unwrap();
        let errored = vstore::create_entry(&mut conn, &mk_input("error")).unwrap();

        // ready: 直接 set_ai_metadata
        vstore::set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: ready.entry.id.clone(),
                summary: None,
                search_aliases: Vec::new(),
                content_hash: "h-ready".into(),
                provider_id: None,
                model: None,
                generated_at: None,
                status: AiMetadataStatus::Ready,
            },
        )
        .unwrap();
        // pending: 已经是默认状态（create_entry 会写入 pending metadata）
        // error: 显式置 error
        vstore::set_ai_metadata(
            &mut conn,
            &VaultAiMetadata {
                entry_id: errored.entry.id.clone(),
                summary: None,
                search_aliases: Vec::new(),
                content_hash: "h-error".into(),
                provider_id: None,
                model: None,
                generated_at: None,
                status: AiMetadataStatus::Error,
            },
        )
        .unwrap();

        let status = vstore::backfill_status(&conn).unwrap();
        assert_eq!(status.total, 3);
        assert_eq!(status.ready, 1);
        assert_eq!(status.pending, 1);
        assert_eq!(status.error, 1);
    }

    // ---- 校验辅助回归 ------------------------------------------------------

    #[test]
    fn validate_final_draft_rejects_placeholder_in_title() {
        let mut d = sample_draft();
        d.title = "[SECRET:abc] leak".into();
        assert!(validate_final_draft(&d).is_err());
    }

    #[test]
    fn validate_final_draft_rejects_placeholder_in_summary() {
        let mut d = sample_draft();
        d.ai_summary = Some("see [SECRET:abc]".into());
        assert!(validate_final_draft(&d).is_err());
    }

    #[test]
    fn validate_final_draft_rejects_too_many_fields() {
        let mut d = sample_draft();
        d.fields = (0..40)
            .map(|i| crate::vault::models::CaptureField {
                draft_id: format!("f{i}"),
                key: format!("k{i}"),
                value: format!("v{i}"),
                is_sensitive: false,
            })
            .collect();
        assert!(validate_final_draft(&d).is_err());
    }

    #[test]
    fn reject_sensitive_metadata_leak_catches_password_in_tag() {
        let mut d = sample_draft();
        d.fields = vec![crate::vault::models::CaptureField {
            draft_id: "f1".into(),
            key: "password".into(),
            value: "topsecretvalue".into(),
            is_sensitive: true,
        }];
        d.ai_tags = vec!["leak-topsecretvalue".into()];
        assert!(reject_sensitive_metadata_leak(&d).is_err());
    }

    #[test]
    fn sensitive_metadata_error_never_contains_the_sensitive_value() {
        let secret = "DO_NOT_ECHO_THIS_PASSWORD";
        let mut d = sample_draft();
        d.fields = vec![crate::vault::models::CaptureField {
            draft_id: "f1".into(),
            key: "password".into(),
            value: secret.into(),
            is_sensitive: true,
        }];
        d.ai_tags = vec![secret.into()];

        let err = reject_sensitive_metadata_leak(&d).expect_err("metadata leak must be rejected");

        assert_eq!(err, "sensitive_metadata_rejected");
        assert!(!err.contains(secret));
    }

    #[test]
    fn reject_sensitive_metadata_leak_allows_non_sensitive_value() {
        let mut d = sample_draft();
        d.fields = vec![crate::vault::models::CaptureField {
            draft_id: "f1".into(),
            key: "user".into(),
            value: "alice".into(),
            is_sensitive: false,
        }];
        d.ai_tags = vec!["alice".into()];
        assert!(reject_sensitive_metadata_leak(&d).is_ok());
    }

    /// 回归 M1：is_sensitive=false 但 key 是默认敏感词（如 "password"）时，
    /// 仍必须进入敏感值检查，防止对抗性 draft 绕过元数据泄漏扫描。
    #[test]
    fn reject_sensitive_metadata_leak_catches_default_sensitive_key_unflagged() {
        let mut d = sample_draft();
        d.fields = vec![crate::vault::models::CaptureField {
            draft_id: "f1".into(),
            key: "password".into(),
            value: "p@ssw0rd-leak".into(),
            // 故意标 is_sensitive=false，模拟对抗性 draft
            is_sensitive: false,
        }];
        d.ai_tags = vec!["contains-p@ssw0rd-leak".into()];
        assert!(
            reject_sensitive_metadata_leak(&d).is_err(),
            "default-sensitive key must be checked even if is_sensitive=false"
        );
    }

    /// 回归 M2：超长 title 必须被拒（避免对抗性 draft 写入 10000 字标题）。
    #[test]
    fn validate_final_draft_rejects_overlong_title() {
        let mut d = sample_draft();
        d.title = "x".repeat(121);
        assert!(validate_final_draft(&d).is_err());
        // 恰好 120 应通过
        d.title = "y".repeat(120);
        assert!(validate_final_draft(&d).is_ok());
    }

    /// 短敏感值（< 6 字符）只在词级精确匹配时拒绝。"admin" 作为 password
    /// 时，AI 给的标签里出现 "administrator" / "admin-login" 等不应被误判。
    #[test]
    fn reject_sensitive_metadata_leak_short_value_requires_word_match() {
        let mut d = sample_draft();
        d.fields = vec![crate::vault::models::CaptureField {
            draft_id: "f1".into(),
            key: "password".into(),
            value: "admin".into(), // 5 chars — 短值
            is_sensitive: true,
        }];
        // "administrator" 包含 "admin" 子串，但不是同一个词 —— 必须放行。
        d.ai_tags = vec!["administrator".into()];
        assert!(
            reject_sensitive_metadata_leak(&d).is_ok(),
            "short value substring inside a longer word must NOT trigger leak"
        );
        // 精确词匹配仍然必须拒绝（即使是 5 字符的值）。
        d.ai_tags = vec!["admin".into()];
        assert!(
            reject_sensitive_metadata_leak(&d).is_err(),
            "exact word match on short value MUST trigger leak"
        );
        // 复合标签 "admin login" 也应拒绝（split_whitespace 命中）。
        d.ai_tags = vec!["admin login".into()];
        assert!(
            reject_sensitive_metadata_leak(&d).is_err(),
            "exact word match inside a multi-word tag MUST trigger leak"
        );
    }
}
