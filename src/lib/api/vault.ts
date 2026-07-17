// src/lib/api/vault.ts
//
// Vault (资料库) IPC 封装。每个方法对应一个在 `src-tauri/src/lib.rs`
// 注册的 `ipc_vault_*` 命令；参数顺序、命名都与 Rust 端 `#[tauri::command]`
// 一致（snake_case 在 Tauri 边界自动转 camelCase 调用参数）。
//
// 新的主 API 与 Task 8-10 的命令一一对齐。下方 `@deprecated` 别名只为
// 让 Task 12-14 之前仍引用旧接口的 Svelte 组件继续 typecheck；后续
// 重构会逐一替换并删除这些别名。

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AiQueryPlan,
  BackfillStatus,
  CaptureDraft,
  CaptureEnrichment,
  EntryKind,
  LlmConfigInput,
  LlmConfigSummary,
  LlmTestResult,
  PlannedSearch,
  ProviderPreset,
  VaultAiSettings,
  VaultEntryDetail,
  VaultEntryInput,
  VaultEntrySummary,
  VaultSearchHit,
  TagUpdateEvent,
  LlmErrorEvent,
} from '$lib/types/vault'

export const vaultApi = {
  // ---- Entry CRUD（Task 10）---------------------------------------------

  createEntry(input: VaultEntryInput): Promise<VaultEntryDetail> {
    return invoke<VaultEntryDetail>('ipc_vault_create_entry', { input })
  },

  updateEntry(id: string, input: VaultEntryInput): Promise<VaultEntryDetail> {
    return invoke<VaultEntryDetail>('ipc_vault_update_entry', { id, input })
  },

  deleteEntry(id: string): Promise<void> {
    return invoke<void>('ipc_vault_delete_entry', { id })
  },

  listEntries(kind?: EntryKind): Promise<VaultEntrySummary[]> {
    return invoke<VaultEntrySummary[]>('ipc_vault_list_entries', {
      kind: kind ?? null,
    })
  },

  getEntry(id: string): Promise<VaultEntryDetail> {
    return invoke<VaultEntryDetail>('ipc_vault_get_entry', { id })
  },

  /**
   * 覆盖式写入 manual tags（AI tags 不动）。返回更新后的 detail。
   */
  updateManualTags(id: string, tags: string[]): Promise<VaultEntryDetail> {
    return invoke<VaultEntryDetail>('ipc_vault_update_manual_tags', { id, tags })
  },

  /**
   * 删除指定的 AI tag（按 normalizedTag 匹配；manual 行不动）。
   */
  removeAiTag(id: string, normalizedTag: string): Promise<VaultEntryDetail> {
    return invoke<VaultEntryDetail>('ipc_vault_remove_ai_tag', { id, normalizedTag })
  },

  /**
   * 重置某条目的 AI metadata 为 pending 并触发后台 backfill。命令立即返回。
   */
  refreshAiMetadata(id: string): Promise<void> {
    return invoke<void>('ipc_vault_refresh_ai_metadata', { id })
  },

  /**
   * 返回当前 DB 中 AI metadata 各状态计数。
   */
  aiBackfillStatus(): Promise<BackfillStatus> {
    return invoke<BackfillStatus>('ipc_vault_ai_backfill_status')
  },

  // ---- Capture（Task 6 + 7 + 10）----------------------------------------

  /**
   * 仅做本地解析，绝不调 LLM。
   */
  parseCaptureLocal(rawText: string): Promise<CaptureDraft> {
    return invoke<CaptureDraft>('ipc_vault_parse_capture_local', { rawText })
  },

  /**
   * 构造脱敏 prompt 调 LLM；返回 suggestion + audit。`manualSensitiveValues`
   * 是录入 UI 中用户额外标记为敏感的值，与 `rawText` 在同一 TokenMap 脱敏。
   */
  enrichCapture(
    draft: CaptureDraft,
    rawText: string,
    manualSensitiveValues: string[],
    requestId: string,
  ): Promise<CaptureEnrichment> {
    return invoke<CaptureEnrichment>('ipc_vault_enrich_capture', {
      draft,
      rawText,
      manualSensitiveValues,
      requestId,
    })
  },

  /**
   * 保存最终 draft 到 DB；幂等（同一 requestId 重复提交返回首次保存结果）；
   * 绝不调 LLM。
   */
  createFromCapture(
    finalDraft: CaptureDraft,
    requestId: string,
  ): Promise<VaultEntryDetail> {
    return invoke<VaultEntryDetail>('ipc_vault_create_from_capture', {
      finalDraft,
      requestId,
    })
  },

  // ---- 混合检索（Task 9）------------------------------------------------

  /**
   * 纯本地混合检索（不调 LLM）。`plan` 由调用方提供（可为 null）。
   */
  searchLocal(
    query: string,
    plan: AiQueryPlan | null,
    limit?: number,
  ): Promise<VaultSearchHit[]> {
    return invoke<VaultSearchHit[]>('ipc_vault_search_hybrid_local', {
      query,
      plan,
      limit: limit ?? null,
    })
  },

  /**
   * 脱敏 query → 调 LLM 生成 AiQueryPlan + 审计。支持 cancelSearch 主动取消；
   * 新调用自动取消旧 active search（同 requestId 例外）。失败时返回本地
   * 降级的空 plan + audit。
   */
  planSearch(query: string, requestId: string): Promise<PlannedSearch> {
    return invoke<PlannedSearch>('ipc_vault_plan_search', { query, requestId })
  },

  /**
   * 取消当前 active search；只在 requestId 匹配时生效（防止迟到 cleanup
   * 误取消新查询）。
   */
  cancelSearch(requestId: string): Promise<void> {
    return invoke<void>('ipc_vault_cancel_search', { requestId })
  },

  // ---- LLM 配置 / AI 设置（Task 8）--------------------------------------

  /**
   * 返回当前已保存的配置概览（不含 API Key）。未配置时返回 null。
   */
  getLlmConfig(): Promise<LlmConfigSummary | null> {
    return invoke<LlmConfigSummary | null>('ipc_vault_get_llm_config')
  },

  /**
   * 用新输入或复用已存 key 测试 LLM 连通性；只有成功才写回 DB 和 runtime。
   * 失败时不覆盖现有 runtime/DB 配置（前端可重试）。
   */
  verifyAndSaveLlm(config: LlmConfigInput): Promise<LlmTestResult> {
    return invoke<LlmTestResult>('ipc_vault_verify_and_save_llm', { config })
  },

  /**
   * 用当前已保存的配置测试连通（用户点 "Test" 时调用）。测试成功会清零
   * 网络失败计数。
   */
  testSavedLlm(): Promise<LlmTestResult> {
    return invoke<LlmTestResult>('ipc_vault_test_saved_llm')
  },

  /**
   * 删除已保存的配置 + 清零所有 runtime 门控状态。
   */
  deleteLlmConfig(): Promise<void> {
    return invoke<void>('ipc_vault_delete_llm_config')
  },

  getAiSettings(): Promise<VaultAiSettings> {
    return invoke<VaultAiSettings>('ipc_vault_get_ai_settings')
  },

  setAiSettings(settings: VaultAiSettings): Promise<VaultAiSettings> {
    return invoke<VaultAiSettings>('ipc_vault_set_ai_settings', { settings })
  },

  /**
   * 返回内置 provider 预设列表（无状态常量）。
   */
  getLlmPresets(): Promise<ProviderPreset[]> {
    return invoke<ProviderPreset[]>('ipc_vault_get_llm_presets')
  },

  // ---- Clipboard（Task 18）-----------------------------------------------

  /**
   * 通过 Tauri 命令复制文本到系统剪贴板。`sensitive = true` 时由后端
   * 从 VaultAiSettings.sensitiveClipboardClearSeconds 读取自动清除秒数
   * （默认 30s）；前端不能伪造更长的清除窗口。
   */
  copyText(text: string, sensitive: boolean): Promise<void> {
    return invoke<void>('ipc_clipboard_copy_text', { text, sensitive })
  },

  // ---- 兼容别名（保留以便外部调用；新代码请使用上面的 typed API） ------
  //
  // 早期迭代中由旧 Svelte 组件（CredentialForm / BookmarkForm / NoteForm /
  // SmartImportDialog / LlmSearchPanel / SearchBar / TagEditor）使用的方法
  // 别名。Task 19 已删除这些组件，但 API 别名本身仍然保留，外部如有引用
  // 不至于 broken。

  /** @deprecated 用 searchLocal() 替代。 */
  search(query: string, limit = 20): Promise<VaultSearchHit[]> {
    return invoke<VaultSearchHit[]>('ipc_vault_search', { query, limit })
  },

  /** @deprecated 改用 planSearch + searchLocal 组合。 */
  llmSearch(query: string, limit = 20): Promise<VaultSearchHit[]> {
    // 旧行为：等价于"原查询 + 无 plan"的本地检索；保留以避免运行时报错。
    return invoke<VaultSearchHit[]>('ipc_vault_search_hybrid_local', {
      query,
      plan: null,
      limit,
    })
  },

  /** @deprecated 用 updateManualTags() 替代。 */
  updateTags(id: string, tags: string[]): Promise<VaultEntryDetail> {
    return invoke<VaultEntryDetail>('ipc_vault_update_manual_tags', { id, tags })
  },

  /** @deprecated 用 refreshAiMetadata() 替代。 */
  retag(id: string): Promise<void> {
    return invoke<void>('ipc_vault_refresh_ai_metadata', { id })
  },

  /** @deprecated 用 verifyAndSaveLlm() 替代；此处保留以避免组件类型错误。 */
  setLlmConfig(config: {
    providerId: string
    baseUrl: string
    apiKey: string
    model: string
  }): Promise<void> {
    return invoke<void>('ipc_vault_verify_and_save_llm', {
      config: {
        providerId: config.providerId,
        baseUrl: config.baseUrl,
        apiKey: config.apiKey,
        model: config.model,
      },
    }).then(() => undefined)
  },

  /** @deprecated 用 verifyAndSaveLlm() 替代。 */
  testLlm(config: {
    providerId: string
    baseUrl: string
    apiKey: string
    model: string
  }): Promise<LlmTestResult> {
    return invoke<LlmTestResult>('ipc_vault_verify_and_save_llm', {
      config: {
        providerId: config.providerId,
        baseUrl: config.baseUrl,
        apiKey: config.apiKey,
        model: config.model,
      },
    })
  },
}

// ---- 事件订阅 ------------------------------------------------------------

export function onTagsUpdated(cb: (e: TagUpdateEvent) => void): Promise<UnlistenFn> {
  return listen<TagUpdateEvent>('vault-tags-updated', (e) => cb(e.payload))
}

export function onLlmError(cb: (e: LlmErrorEvent) => void): Promise<UnlistenFn> {
  return listen<LlmErrorEvent>('vault-llm-error', (e) => cb(e.payload))
}
