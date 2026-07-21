// src/lib/types/vault.ts
//
// Vault (资料库) 前端类型定义。与 Rust 端 `src-tauri/src/vault/models.rs`
// 以及 IPC 命令逐字段对齐，所有 struct 都使用 camelCase 序列化。
//
// 这些类型覆盖：
//   * 条目 CRUD（Task 10）：`VaultEntry`、`VaultField`、`VaultTag`、
//     `VaultEntryDetail`、`VaultEntrySummary`、`VaultEntryInput`、`FieldInput`。
//   * AI 元数据：`VaultAiMetadata`、`AiMetadataStatus`、`TagSource`、
//     `BackfillStatus`。
//   * 录入流程（Task 6 + 7 + 10）：`CaptureField`、`CaptureDraft`、
//     `SuggestedField`、`CaptureSuggestion`、`AiProvenance`、`AuditMessage`、
//     `AiRequestAudit`、`CaptureEnrichment`、`CaptureWarning`（沿用 string[]）。
//   * 混合检索（Task 9）：`AiQueryPlan`、`SearchSource`、`VaultSearchHit`、
//     `PlannedSearch`。
//   * LLM 配置 / AI 设置（Task 8）：`LlmConfigInput`、`LlmConfigSummary`、
//     `VaultAiSettings`、`LlmTestResult`、`ProviderPreset`。

// ---- 基础枚举 -------------------------------------------------------------

/** 条目类型。Rust 端 `EntryKind` 以 lowercase 序列化。 */
export type EntryKind = 'credential' | 'bookmark' | 'note'

/** Tag 来源：用户手填或 LLM 生成。 */
export type TagSource = 'manual' | 'ai'

/** AI 元数据状态：pending → ready / error。 */
export type AiMetadataStatus = 'ready' | 'pending' | 'error'

/**
 * 检索命中来源。
 *
 * Rust 端 enum 标注 `#[serde(rename_all = "camelCase")]`，因此 `Local` ->
 * `'local'`，`AiExpanded` -> `'aiExpanded'`。
 */
export type SearchSource = 'local' | 'aiExpanded'

// ---- 条目相关 -------------------------------------------------------------

export interface VaultEntry {
  id: string
  kind: EntryKind
  title: string
  notes: string | null
  createdAt: string
  updatedAt: string
}

export interface VaultField {
  id: string
  entryId: string
  key: string
  value: string
  isSensitive: boolean
  sortOrder: number
}

export interface VaultTag {
  tag: string
  normalizedTag: string
  source: TagSource
}

export interface VaultAiMetadata {
  entryId: string
  summary: string | null
  searchAliases: string[]
  contentHash: string
  providerId: string | null
  model: string | null
  generatedAt: string | null
  status: AiMetadataStatus
}

export interface FieldInput {
  key: string
  value: string
  isSensitive: boolean
}

/**
 * 创建 / 更新条目的输入。`manualTags` 必须显式传入（即便为空数组），
 * 因为 Rust 端按整字段覆盖语义写库。
 */
export interface VaultEntryInput {
  kind: EntryKind
  title: string
  fields: FieldInput[]
  notes: string | null
  manualTags: string[]
}

export interface VaultEntryDetail {
  entry: VaultEntry
  fields: VaultField[]
  tags: VaultTag[]
  aiMetadata: VaultAiMetadata | null
}

export interface VaultEntrySummary {
  entry: VaultEntry
  tags: VaultTag[]
  preview: string | null
}

// ---- 检索相关 -------------------------------------------------------------

export interface VaultSearchHit {
  summary: VaultEntrySummary
  score: number
  sources: SearchSource[]
}

/**
 * LLM 生成的查询计划。所有字段都允许为空（缺配置或 LLM 失败时降级）。
 */
export interface AiQueryPlan {
  kinds: EntryKind[]
  keywords: string[]
  aliases: string[]
  dateFrom: string | null
  dateTo: string | null
}

export interface AuditMessage {
  role: string
  content: string
}

export interface AiRequestAudit {
  providerId: string
  model: string
  sentAt: string
  messages: AuditMessage[]
}

export interface PlannedSearch {
  plan: AiQueryPlan
  understoodTerms: string[]
  audit: AiRequestAudit
}

// ---- 录入（capture）相关 -------------------------------------------------

export interface CaptureField {
  draftId: string
  key: string
  value: string
  isSensitive: boolean
}

export interface AiProvenance {
  providerId: string
  model: string
  generatedAt: string
}

export interface CaptureDraft {
  kind: EntryKind
  title: string
  notes: string | null
  fields: CaptureField[]
  manualTags: string[]
  aiTags: string[]
  aiSummary: string | null
  searchAliases: string[]
  aiProvenance: AiProvenance | null
  warnings: string[]
}

export interface SuggestedField {
  key: string
  value: string
  isSensitive: boolean
}

export interface CaptureSuggestion {
  kind: EntryKind | null
  title: string | null
  notes: string | null
  fields: SuggestedField[]
  aiTags: string[]
  aiSummary: string | null
  searchAliases: string[]
}

export interface CaptureEnrichment {
  suggestion: CaptureSuggestion
  audit: AiRequestAudit
}

// ---- 后台任务状态 --------------------------------------------------------

export interface BackfillStatus {
  total: number
  pending: number
  processing: number
  ready: number
  error: number
}

// ---- LLM 配置 / AI 设置 --------------------------------------------------

/**
 * verify-and-save 输入。`apiKey` 为 `null`/空字符串表示"复用已存 key"
 * （仅在 provider 未变时合法）。
 */
export interface LlmConfigInput {
  providerId: string
  baseUrl: string
  apiKey: string | null
  model: string
}

/**
 * 已保存配置的只读视图。**绝不包含 API Key**，只有 `hasApiKey` 标志位。
 */
export interface LlmConfigSummary {
  providerId: string
  baseUrl: string
  model: string
  hasApiKey: boolean
}

export interface VaultAiSettings {
  autoEnrich: boolean
  autoHybridSearch: boolean
  thinkingEnabled: boolean
  sensitiveClipboardClearSeconds: number | null
}

/**
 * LLM 连通性测试结果。
 *
 * 与 Rust 端 `LlmTestResult` 对齐：`ok`/`message`/`modelEcho`。
 */
export interface LlmTestResult {
  ok: boolean
  message: string
  modelEcho: string | null
}

export interface ProviderPreset {
  id: string
  label: string
  baseUrl: string
  models: string[]
  defaultModel: string
}

// ---- 事件 ----------------------------------------------------------------

/**
 * LLM 错误事件 payload。
 */
export interface LlmErrorEvent {
  kind: string
  code: string
}
