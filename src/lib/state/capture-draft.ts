// src/lib/state/capture-draft.ts
//
// CaptureDraftController —— Vault 录入流程的前端状态协调器。
//
// 行为契约（与 Task 6/7/10 IPC 对齐）：
//   * `startSession()` 生成新的 requestId（`crypto.randomUUID()`），并清空 dirty
//     标记。
//   * `setLocalDraft(d)` 来自 `parseCaptureLocal` 的初始 draft；不标任何路径
//     dirty（视为 LLM 的基线输入）。
//   * `setTitle/Notes/Kind/FieldKey/FieldValue/...` 把对应路径标 dirty 并写入
//     draft。dirty path 永远不会被后续 enrich 覆盖。
//   * `enrich(rawText, manualSensitiveValues)` 调用 `enrichCapture` 并把
//     suggestion 合并到 draft：
//     - title/notes/kind 仅在未 dirty 时覆盖；
//     - 同 key 的 field 不重复追加；已存在的非 dirty field 的 value 会被覆盖；
//       dirty field 不动；
//     - 任何新 key 的 field 追加到末尾；
//     - 成功合并后从 audit 写入 `aiProvenance`。
//   * `save()` 调用 `createFromCapture(draft, requestId)`。失败时 requestId 不
//     变（可重试）；成功后才在内部生成新 session id（下次操作用新 id）。
//
// dirty 路径用 Set<string> 表示，命名约定：
//   - `title`
//   - `notes`
//   - `kind`
//   - `field:<draftId>:key`
//   - `field:<draftId>:value`
//   - `field:<draftId>:sensitive`
//   - `manualTags`（整体覆盖语义；任一编辑即整体 dirty）

import type {
  AiProvenance,
  CaptureDraft,
  CaptureEnrichment,
  CaptureField,
  EntryKind,
  VaultEntryDetail,
} from '$lib/types/vault'

/** Capture IPC 表面：与 `vaultApi` 子集一致，便于 mock。 */
export interface CaptureDraftApi {
  enrichCapture(
    draft: CaptureDraft,
    rawText: string,
    manualSensitiveValues: string[],
    requestId: string,
  ): Promise<CaptureEnrichment>
  createFromCapture(
    finalDraft: CaptureDraft,
    requestId: string,
  ): Promise<VaultEntryDetail>
}

export interface CaptureDraftControllerOptions {
  api: CaptureDraftApi
}

const TITLE_PATH = 'title'
const NOTES_PATH = 'notes'
const KIND_PATH = 'kind'
const MANUAL_TAGS_PATH = 'manualTags'

function fieldKeyPath(draftId: string): string {
  return `field:${draftId}:key`
}

function fieldValuePath(draftId: string): string {
  return `field:${draftId}:value`
}

function fieldSensitivePath(draftId: string): string {
  return `field:${draftId}:sensitive`
}

/**
 * 录入流程协调器。一个实例对应一次完整的"粘贴 → 预览 → 编辑 → 保存"流程；
 * 成功保存后自动开新 session。
 */
export class CaptureDraftController {
  private api: CaptureDraftApi
  private _requestId: string | null = null
  private _draft: CaptureDraft | null = null
  private dirty = new Set<string>()

  constructor(opts: CaptureDraftControllerOptions) {
    this.api = opts.api
  }

  /** 当前 session 的 request id；未启动时为 null。 */
  get requestId(): string | null {
    return this._requestId
  }

  /** 当前 draft；未设置时抛错（调用方应先 setLocalDraft）。 */
  get draft(): CaptureDraft {
    return this.req()
  }

  /** 开启新的 capture session；生成新 requestId 并清空 dirty。 */
  startSession(): string {
    const id = crypto.randomUUID()
    this._requestId = id
    this.dirty.clear()
    return id
  }

  /**
   * 设置初始的本地 draft（来自 `parseCaptureLocal`）。不会标任何路径 dirty。
   * 若需要从头开始一个 session，调用方应先 startSession()。
   */
  setLocalDraft(draft: CaptureDraft): void {
    this._draft = cloneDraft(draft)
    // dirty 不动；setLocalDraft 是基线快照。
  }

  // ---- 用户编辑入口 ------------------------------------------------------

  setTitle(value: string): void {
    const d = this.req()
    d.title = value
    this.dirty.add(TITLE_PATH)
  }

  setNotes(value: string | null): void {
    const d = this.req()
    d.notes = value
    this.dirty.add(NOTES_PATH)
  }

  setKind(kind: EntryKind): void {
    const d = this.req()
    d.kind = kind
    this.dirty.add(KIND_PATH)
  }

  setFieldKey(draftId: string, key: string): void {
    const f = this.findField(draftId)
    if (!f) return
    f.key = key
    this.dirty.add(fieldKeyPath(draftId))
  }

  setFieldValue(draftId: string, value: string): void {
    const f = this.findField(draftId)
    if (!f) return
    f.value = value
    this.dirty.add(fieldValuePath(draftId))
  }

  setFieldSensitive(draftId: string, isSensitive: boolean): void {
    const f = this.findField(draftId)
    if (!f) return
    f.isSensitive = isSensitive
    // 敏感切换不进入"值"dirty 路径；但仍然认为该字段已被用户操作。
    this.dirty.add(fieldSensitivePath(draftId))
  }

  addField(key: string, value: string, isSensitive: boolean): CaptureField {
    const d = this.req()
    const draftId = crypto.randomUUID()
    const field: CaptureField = { draftId, key, value, isSensitive }
    d.fields.push(field)
    return field
  }

  removeField(draftId: string): void {
    const d = this.req()
    const before = d.fields.length
    d.fields = d.fields.filter((f) => f.draftId !== draftId)
    if (d.fields.length !== before) {
      // 清理对应的 dirty 标记，避免遗留。
      this.dirty.delete(fieldKeyPath(draftId))
      this.dirty.delete(fieldValuePath(draftId))
      this.dirty.delete(fieldSensitivePath(draftId))
    }
  }

  setManualTags(tags: string[]): void {
    const d = this.req()
    d.manualTags = [...tags]
    this.dirty.add(MANUAL_TAGS_PATH)
  }

  // ---- AI 增强 -----------------------------------------------------------

  /**
   * 调用 enrichCapture 并把 suggestion 合并到 draft。dirty 路径不会被覆盖。
   * 成功后从 audit 写入 aiProvenance；失败抛出原错误，draft 不变。
   */
  async enrich(rawText: string, manualSensitiveValues: string[]): Promise<void> {
    if (this._requestId === null) {
      // 自动开 session（方便单测和简单调用方）。
      this.startSession()
    }
    const snapshot = cloneDraft(this.req())
    const enr: CaptureEnrichment = await this.api.enrichCapture(
      snapshot,
      rawText,
      manualSensitiveValues,
      this._requestId!,
    )
    this.applyEnrichment(enr)
  }

  /**
   * 把已合并的 draft 保存到 DB。失败时 requestId 不变；成功后内部生成新
   * session（下次 save / enrich 用新 id）。
   */
  async save(): Promise<VaultEntryDetail> {
    if (this._requestId === null) {
      this.startSession()
    }
    const requestId = this._requestId!
    const result = await this.api.createFromCapture(cloneDraft(this.req()), requestId)
    // 成功 → 生成新 session id（不清空 draft，调用方可决定是否重置）。
    this._requestId = crypto.randomUUID()
    this.dirty.clear()
    return result
  }

  // ---- internal ----------------------------------------------------------

  /** Returns the current draft (throws if not set). */
  private req(): CaptureDraft {
    if (this._draft === null) {
      throw new Error('CaptureDraftController: draft is null (call setLocalDraft first)')
    }
    return this._draft
  }

  private findField(draftId: string): CaptureField | undefined {
    return this.req().fields.find((f) => f.draftId === draftId)
  }

  private applyEnrichment(enr: CaptureEnrichment): void {
    const suggestion = enr.suggestion
    const draft = this.req()

    // kind
    if (suggestion.kind !== null && !this.dirty.has(KIND_PATH)) {
      draft.kind = suggestion.kind
    }
    // title
    if (suggestion.title !== null && !this.dirty.has(TITLE_PATH)) {
      draft.title = suggestion.title
    }
    // notes
    if (suggestion.notes !== null && !this.dirty.has(NOTES_PATH)) {
      draft.notes = suggestion.notes
    }
    // fields
    if (suggestion.fields.length > 0) {
      const byKey = new Map<string, CaptureField>()
      for (const f of draft.fields) {
        if (!byKey.has(f.key)) byKey.set(f.key, f)
      }
      for (const sug of suggestion.fields) {
        const existing = byKey.get(sug.key)
        if (existing) {
          // 同 key：已存在的字段，如果未 dirty，则覆盖 value（不复制新行）。
          if (!this.dirty.has(fieldValuePath(existing.draftId))) {
            existing.value = sug.value
          }
          if (!this.dirty.has(fieldSensitivePath(existing.draftId))) {
            existing.isSensitive = sug.isSensitive
          }
        } else {
          // 新 key：追加。
          const draftId = crypto.randomUUID()
          const newField: CaptureField = {
            draftId,
            key: sug.key,
            value: sug.value,
            isSensitive: sug.isSensitive,
          }
          draft.fields.push(newField)
          byKey.set(sug.key, newField)
        }
      }
    }
    // aiTags / aiSummary / searchAliases —— 这些是 LLM 产出的元数据，每次
    // enrich 都覆盖（不进入用户 dirty 路径；用户如需修改可手动编辑后再次
    // enrich）。
    if (suggestion.aiTags.length > 0) draft.aiTags = [...suggestion.aiTags]
    if (suggestion.aiSummary !== null) draft.aiSummary = suggestion.aiSummary
    if (suggestion.searchAliases.length > 0) draft.searchAliases = [...suggestion.searchAliases]

    // aiProvenance from audit.
    const audit = enr.audit
    const provenance: AiProvenance = {
      providerId: audit.providerId,
      model: audit.model,
      generatedAt: audit.sentAt,
    }
    draft.aiProvenance = provenance
  }
}

// ---- helpers --------------------------------------------------------------

function cloneDraft(d: CaptureDraft): CaptureDraft {
  return {
    kind: d.kind,
    title: d.title,
    notes: d.notes,
    fields: d.fields.map((f) => ({ ...f })),
    manualTags: [...d.manualTags],
    aiTags: [...d.aiTags],
    aiSummary: d.aiSummary,
    searchAliases: [...d.searchAliases],
    aiProvenance: d.aiProvenance ? { ...d.aiProvenance } : null,
    warnings: [...d.warnings],
  }
}
