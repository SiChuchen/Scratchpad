<script lang="ts">
  // src/lib/components/quick-access/CaptureMode.svelte
  //
  // 全局"记录"模式：粘贴/输入 → 本地解析 → (可选) AI 整理 → 预览编辑 → 保存。
  //
  // 时序（与 Task 16 spec 一致）：
  //   * rawText 变化 → 取消旧 timer，200ms 后调用 parseCaptureLocal，立即
  //     发布 preview（不等 AI）。
  //   * 本地解析成功 + autoEnrich + aiConfigured → 500ms 后调用 controller.enrich
  //     （内部调 enrichCapture 并按 dirty 路径合并 suggestion 到 draft）。
  //   * 每次 rawText 变化自增 captureRevision，迟到的异步结果（旧 revision）
  //     被丢弃，避免预览闪烁。
  //   * 用户编辑通过 CaptureDraftController 的 setter 写入并自动标 dirty，
  //     AI 合并时不会覆盖 dirty 路径。本组件另外维护一份 dirtyPaths 集合，
  //     用于在 enrich 后计算 "partial" 计数。
  //   * 失败状态（AI 或 save）只显示错误，不禁用保存按钮——AI 未配置时
  //     显示设置入口但仍允许保存本地解析结果。
  //
  // Ctrl+Enter 触发 save；按钮点击也走同一个 pending guard。

  import { onMount, onDestroy } from 'svelte'
  import { vaultApi } from '$lib/api/vault'
  import { messages } from '$lib/i18n'
  import { CaptureDraftController } from '$lib/state/capture-draft'
  import type {
    CaptureDraft,
    CaptureEnrichment,
    EntryKind,
  } from '$lib/types/vault'

  interface Props {
    notify: (
      text: string,
      kind?: 'success' | 'error',
      undo?: () => void,
      actionLabel?: string,
    ) => void
    /** AI 是否已配置（来自 QuickAccessApp 重读，避免 stale 快照）。 */
    aiConfigured?: boolean
    /** AI 自动整理开关（来自 QuickAccessApp 重读）。 */
    autoEnrich?: boolean
    onSaved?: (id: string) => void
    onOpenSettings?: () => void
  }

  let {
    notify,
    aiConfigured = false,
    autoEnrich = true,
    onSaved,
    onOpenSettings,
  }: Props = $props()

  // ---- Constants ----------------------------------------------------------

  const LOCAL_PARSE_DELAY_MS = 200
  const AI_ENRICH_DELAY_MS = 500

  // ---- State --------------------------------------------------------------

  let rawText = $state('')
  let controller = new CaptureDraftController({ api: vaultApi })
  let draft = $state<CaptureDraft | null>(null)
  let enrichment = $state<CaptureEnrichment | null>(null)
  let enrichStatus = $state<'idle' | 'enriching' | 'merged' | 'partial' | 'failed'>('idle')
  let enrichFailureDetail = $state('')
  let partialCount = $state(0)
  let auditOpen = $state(false)
  let manualSensitiveValues = $state<string[]>([])
  let saving = $state(false)
  let saveError = $state<string | null>(null)

  let requestId = $state<string>('')
  let captureRevision = $state(0)

  // Component-tracked dirty paths so we can compute "partial" enrichment status
  // after controller.enrich. The controller has its own private dirty Set for
  // merge logic; this set mirrors user edits for UI reporting only.
  let dirtyPaths = $state<Set<string>>(new Set())

  let parseTimer: ReturnType<typeof setTimeout> | null = null
  let enrichTimer: ReturnType<typeof setTimeout> | null = null

  // ---- Lifecycle ----------------------------------------------------------

  onMount(async () => {
    // 开启首个 session，预先生成 requestId 供 enrich / save 使用。
    requestId = controller.startSession()
  })

  onDestroy(() => {
    if (parseTimer) clearTimeout(parseTimer)
    if (enrichTimer) clearTimeout(enrichTimer)
  })

  // ---- Helpers ------------------------------------------------------------

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

  function markDirty(path: string) {
    const next = new Set(dirtyPaths)
    next.add(path)
    dirtyPaths = next
  }

  function resetDirty() {
    dirtyPaths = new Set()
  }

  // ---- Input debouncing ---------------------------------------------------

  function onRawTextInput(e: Event) {
    const value = (e.currentTarget as HTMLTextAreaElement).value
    rawText = value
    if (parseTimer) clearTimeout(parseTimer)
    if (enrichTimer) {
      clearTimeout(enrichTimer)
      enrichTimer = null
    }
    if (!value.trim()) {
      // Empty input: clear preview.
      draft = null
      enrichment = null
      enrichStatus = 'idle'
      enrichFailureDetail = ''
      resetDirty()
      return
    }
    parseTimer = setTimeout(() => {
      void doLocalParse()
    }, LOCAL_PARSE_DELAY_MS)
  }

  async function doLocalParse() {
    if (!rawText.trim()) return
    const myRevision = ++captureRevision
    try {
      const parsed = await vaultApi.parseCaptureLocal(rawText)
      if (myRevision !== captureRevision) return // stale
      controller.setLocalDraft(parsed)
      // Reset dirty paths because the user hasn't edited the new baseline yet.
      resetDirty()
      draft = cloneDraft(controller.draft)
      // AI enrich scheduling
      if (autoEnrich && aiConfigured) {
        enrichStatus = 'idle'
        enrichFailureDetail = ''
        if (enrichTimer) clearTimeout(enrichTimer)
        enrichTimer = setTimeout(() => {
          void doEnrich()
        }, AI_ENRICH_DELAY_MS)
      }
    } catch (e) {
      console.error('CaptureMode: local parse failed', e)
    }
  }

  async function doEnrich() {
    if (!draft) return
    if (!autoEnrich || !aiConfigured) return
    const myRevision = ++captureRevision
    enrichStatus = 'enriching'
    try {
      // controller.enrich calls vaultApi.enrichCapture (via api), merges
      // suggestion honoring dirty paths, and returns the enrichment (incl.
      // audit) so we can populate the audit dialog.
      const result = await controller.enrich(rawText, [...manualSensitiveValues])
      if (myRevision !== captureRevision) return // stale

      partialCount = countDirtySuggestions()
      enrichStatus = partialCount > 0 ? 'partial' : 'merged'

      draft = cloneDraft(controller.draft)
      enrichment = result
    } catch (e) {
      console.error('CaptureMode: enrich failed', e)
      enrichStatus = 'failed'
      enrichFailureDetail = describeEnrichFailure(e)
      // Save still enabled — preview from local parse is intact.
    }
  }

  function describeEnrichFailure(error: unknown): string {
    const message = error instanceof Error ? error.message : String(error)
    if (message.includes('response truncated')) return messages.quickAccess.aiTruncated
    if (message.includes('authentication failed')) return messages.quickAccess.aiAuthFailed
    if (message.includes('rate limited')) return messages.quickAccess.aiRateLimited
    if (message.includes('timeout')) return messages.quickAccess.aiTimedOut
    if (message.includes('network error')) return messages.quickAccess.aiNetworkFailed
    return ''
  }

  function countDirtySuggestions(): number {
    // For each suggestion-type path that user marked dirty, count as "partial"
    // (could not apply). title/notes/kind/fields/* are candidates.
    let n = 0
    for (const p of ['title', 'notes', 'kind']) {
      if (dirtyPaths.has(p)) n++
    }
    // field dirty paths use prefix "field:" — count unique draftIds touched.
    const fieldIds = new Set<string>()
    for (const p of dirtyPaths) {
      if (p.startsWith('field:')) {
        const parts = p.split(':')
        if (parts.length >= 2) fieldIds.add(parts[1]!)
      }
    }
    n += fieldIds.size
    return n
  }

  // ---- User editing -------------------------------------------------------

  function onTitleInput(e: Event) {
    if (!draft) return
    const v = (e.currentTarget as HTMLInputElement).value
    controller.setTitle(v)
    markDirty('title')
    draft = cloneDraft(controller.draft)
  }

  function onNotesInput(e: Event) {
    if (!draft) return
    const v = (e.currentTarget as HTMLTextAreaElement).value
    controller.setNotes(v)
    markDirty('notes')
    draft = cloneDraft(controller.draft)
  }

  function onKindChange(e: Event) {
    if (!draft) return
    const v = (e.currentTarget as HTMLSelectElement).value as EntryKind
    controller.setKind(v)
    markDirty('kind')
    draft = cloneDraft(controller.draft)
  }

  function onFieldKeyInput(draftId: string, e: Event) {
    if (!draft) return
    const v = (e.currentTarget as HTMLInputElement).value
    controller.setFieldKey(draftId, v)
    markDirty(`field:${draftId}:key`)
    draft = cloneDraft(controller.draft)
  }

  function onFieldValueInput(draftId: string, e: Event) {
    if (!draft) return
    const v = (e.currentTarget as HTMLInputElement).value
    controller.setFieldValue(draftId, v)
    markDirty(`field:${draftId}:value`)
    draft = cloneDraft(controller.draft)
  }

  function onFieldSensitiveToggle(draftId: string, e: Event) {
    if (!draft) return
    const checked = (e.currentTarget as HTMLInputElement).checked
    controller.setFieldSensitive(draftId, checked)
    markDirty(`field:${draftId}:sensitive`)
    draft = cloneDraft(controller.draft)
  }

  function onAddField() {
    controller.addField('', '', false)
    draft = cloneDraft(controller.draft)
  }

  function onRemoveField(draftId: string) {
    controller.removeField(draftId)
    draft = cloneDraft(controller.draft)
  }

  function onManualTagsInput(e: Event) {
    const v = (e.currentTarget as HTMLInputElement).value
    const tags = v
      .split(',')
      .map((t) => t.trim())
      .filter(Boolean)
    controller.setManualTags(tags)
    markDirty('manualTags')
    draft = cloneDraft(controller.draft)
  }

  function onRemoveAiTag(tag: string) {
    if (!draft) return
    // Spec: removing AI tag only removes from draft.aiTags (not stored in DB).
    draft.aiTags = draft.aiTags.filter((t) => t !== tag)
    controller.setLocalDraft(draft)
    draft = cloneDraft(controller.draft)
  }

  function onConvertAiTag(tag: string) {
    if (!draft) return
    // Spec: modifying AI tag = remove original + add new value to manualTags.
    // "→ 手动" affordance: move the tag value verbatim from aiTags into
    // manualTags, then drop it from aiTags so it is no longer tied to AI
    // provenance (and won't be overwritten by a future enrich cycle).
    if (!draft.aiTags.includes(tag)) return
    if (!draft.manualTags.includes(tag)) {
      draft.manualTags = [...draft.manualTags, tag]
    }
    draft.aiTags = draft.aiTags.filter((t) => t !== tag)
    controller.setLocalDraft(draft)
    controller.setManualTags(draft.manualTags)
    markDirty('manualTags')
    draft = cloneDraft(controller.draft)
  }

  // ---- Sensitive marking --------------------------------------------------

  function onMarkSensitive() {
    const selection =
      (typeof window !== 'undefined' && window.getSelection()?.toString()) ?? ''
    if (selection && !manualSensitiveValues.includes(selection)) {
      manualSensitiveValues = [...manualSensitiveValues, selection]
    }
  }

  function onRemoveSensitiveMark(value: string) {
    manualSensitiveValues = manualSensitiveValues.filter((v) => v !== value)
  }

  // ---- Save flow ----------------------------------------------------------

  async function save() {
    if (saving) return
    if (!draft) return
    saving = true
    saveError = null
    try {
      const entry = await vaultApi.createFromCapture(draft, requestId)
      notify(messages.quickAccess.saved, 'success')
      onSaved?.(`vault:${entry.entry.id}`)
      // Rotate session and reset state.
      requestId = controller.startSession()
      rawText = ''
      draft = null
      enrichment = null
      enrichStatus = 'idle'
      enrichFailureDetail = ''
      partialCount = 0
      manualSensitiveValues = []
      resetDirty()
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e)
      saveError = raw === 'sensitive_metadata_rejected'
        ? messages.quickAccess.sensitiveMetadataRejected
        : raw
      // rawText, draft, requestId all preserved — the storage layer's
      // vault_capture_requests idempotency check guarantees the retry of the
      // SAME requestId won't create a duplicate entry if the first attempt
      // actually persisted before the failure surfaced.
    } finally {
      saving = false
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.ctrlKey && e.key === 'Enter') {
      e.preventDefault()
      void save()
    }
  }

  // ---- Derived ------------------------------------------------------------

  const canSave = $derived(!!draft && !saving)
  const partialStatusText = $derived(
    enrichStatus === 'partial'
      ? `${messages.quickAccess.aiMerged} (${partialCount})`
      : '',
  )
</script>

<div class="capture-mode">
  <header class="mode-header">
    <h2>{messages.quickAccess.record}</h2>
    <span class="hint">Ctrl+Enter {messages.quickAccess.save} · Ctrl+Tab {messages.quickAccess.search}</span>
  </header>

  {#if !aiConfigured || !autoEnrich}
    <div class="ai-status-banner" role="status" aria-live="polite">
      <span class="ai-status-icon">⚠</span>
      <span class="ai-status-text">
        {aiConfigured
          ? messages.quickAccess.autoEnrichDisabled
          : messages.quickAccess.aiNotConfigured}
      </span>
      <button type="button" class="ghost-btn small" onclick={onOpenSettings}>
        {messages.quickAccess.configureNow}
      </button>
    </div>
  {/if}

  <textarea
    class="raw-textarea"
    placeholder={messages.quickAccess.inputPlaceholder}
    value={rawText}
    oninput={onRawTextInput}
    onkeydown={onKeydown}
    aria-label={messages.quickAccess.record}
  ></textarea>

  {#if manualSensitiveValues.length > 0}
    <div class="sensitive-marks">
      <span class="marks-label">{messages.quickAccess.markSensitive}：</span>
      {#each manualSensitiveValues as value (value)}
        <span class="mark">
          <span class="mark-text">{value}</span>
          <button
            type="button"
            class="mark-remove"
            aria-label={`${messages.quickAccess.removeSensitiveMark} ${value}`}
            title={messages.quickAccess.removeSensitiveMark}
            onclick={() => onRemoveSensitiveMark(value)}
          >×</button>
        </span>
      {/each}
    </div>
  {/if}

  {#if rawText.trim()}
    <button type="button" class="ghost-btn" onclick={onMarkSensitive}>
      {messages.quickAccess.markSensitive}
    </button>
  {/if}

  {#if enrichStatus === 'enriching'}
    <div class="enrich-status" aria-live="polite">{messages.quickAccess.aiEnhancing}…</div>
  {:else if enrichStatus === 'merged'}
    <div class="enrich-status merged" aria-live="polite">{messages.quickAccess.aiMerged}</div>
  {:else if enrichStatus === 'partial'}
    <div class="enrich-status partial" aria-live="polite">{partialStatusText}</div>
  {:else if enrichStatus === 'failed'}
    <div class="enrich-status failed" aria-live="polite">
      {messages.quickAccess.aiFallback}{enrichFailureDetail ? `：${enrichFailureDetail}` : ''}
    </div>
  {/if}

  {#if draft}
    <div class="preview">
      <label class="form-row">
        <span class="row-label">{messages.library.kind}</span>
        <select class="kind-select" value={draft.kind} onchange={onKindChange} aria-label={messages.library.kind}>
          <option value="note">{messages.library.note}</option>
          <option value="credential">{messages.library.credential}</option>
          <option value="bookmark">{messages.library.bookmark}</option>
        </select>
      </label>

      <label class="form-row">
        <span class="row-label">{messages.library.titleLabel}</span>
        <input
          class="text-input"
          type="text"
          value={draft.title}
          oninput={onTitleInput}
          aria-label={messages.library.titleLabel}
        />
      </label>

      <label class="form-row">
        <span class="row-label">{messages.library.notesLabel}</span>
        <textarea
          class="notes-input"
          value={draft.notes ?? ''}
          oninput={onNotesInput}
          aria-label={messages.library.notesLabel}
        ></textarea>
      </label>

      <div class="fields-section">
        <div class="section-label">
          <span>{messages.library.fieldsLabel}</span>
          <button type="button" class="ghost-btn small" onclick={onAddField}>
            + {messages.library.addField}
          </button>
        </div>
        {#each draft.fields as field (field.draftId)}
          <div class="field-row">
            <input
              class="text-input field-key"
              type="text"
              placeholder={messages.library.fieldKeyPlaceholder}
              value={field.key}
              oninput={(e) => onFieldKeyInput(field.draftId, e)}
              aria-label={messages.library.fieldKeyPlaceholder}
            />
            <input
              class="text-input field-value"
              type="text"
              placeholder={messages.library.fieldValuePlaceholder}
              value={field.value}
              oninput={(e) => onFieldValueInput(field.draftId, e)}
              aria-label={messages.library.fieldValuePlaceholder}
            />
            <label class="sensitive-toggle">
              <input
                type="checkbox"
                checked={field.isSensitive}
                onchange={(e) => onFieldSensitiveToggle(field.draftId, e)}
              />
              <span>{messages.quickAccess.markSensitive}</span>
            </label>
            <button
              type="button"
              class="ghost-btn small danger"
              aria-label={messages.library.delete}
              title={messages.library.delete}
              onclick={() => onRemoveField(field.draftId)}
            >×</button>
          </div>
        {/each}
      </div>

      <label class="form-row">
        <span class="row-label">{messages.library.manualTag}</span>
        <input
          class="text-input"
          type="text"
          placeholder={messages.library.tagsPlaceholder}
          value={draft.manualTags.join(', ')}
          oninput={onManualTagsInput}
          aria-label={messages.library.manualTag}
        />
      </label>

      {#if draft.aiTags.length > 0}
        <div class="tags-section">
          <span class="row-label">{messages.library.aiTag}</span>
          <div class="tag-list">
            {#each draft.aiTags as tag (tag)}
              <span class="tag ai-tag">
                {tag}
                <button
                  type="button"
                  class="tag-convert"
                  aria-label={`${messages.quickAccess.convertToManualPrefix}${tag}`}
                  title={messages.library.manualTag}
                  onclick={() => onConvertAiTag(tag)}
                >→{messages.library.manualTag}</button>
                <button
                  type="button"
                  class="tag-remove"
                  aria-label={`${messages.quickAccess.removeAiTagPrefix}${tag}`}
                  title={messages.library.delete}
                  onclick={() => onRemoveAiTag(tag)}
                >×</button>
              </span>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {/if}

  <div class="actions-row">
    {#if enrichment}
      <button type="button" class="ghost-btn" onclick={() => (auditOpen = true)}>
        {messages.quickAccess.outboundAudit}
      </button>
    {/if}
    <button
      type="button"
      class="primary-btn"
      onclick={save}
      disabled={!canSave}
    >
      {saving ? messages.quickAccess.saving : messages.quickAccess.save}
    </button>
  </div>

  {#if saveError}
    <div class="error-banner" role="alert">{saveError}</div>
  {/if}

  {#if auditOpen && enrichment}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="audit-overlay" role="dialog" aria-label={messages.quickAccess.outboundAudit}>
      <div class="audit-dialog">
        <header class="audit-header">
          <h3>{messages.quickAccess.outboundAudit}</h3>
          <button
            type="button"
            class="ghost-btn small"
            aria-label={messages.quickAccess.close}
            title={messages.quickAccess.close}
            onclick={() => (auditOpen = false)}
          >{messages.quickAccess.close}</button>
        </header>
        <div class="audit-meta">
          <span>{messages.quickAccess.auditProvider}: {enrichment.audit.providerId}</span>
          <span>{messages.quickAccess.auditModel}: {enrichment.audit.model}</span>
          <span>{messages.quickAccess.auditSentAt}: {enrichment.audit.sentAt}</span>
        </div>
        <div class="audit-messages">
          {#each enrichment.audit.messages as msg, i (i)}
            <div class="audit-msg">
              <strong>{msg.role}</strong>
              <pre>{msg.content}</pre>
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .capture-mode {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.75rem;
    min-height: 0;
  }

  .ai-status-banner {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.6rem;
    border-radius: 6px;
    background: color-mix(in srgb, var(--color-warning, #f59e0b) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning, #f59e0b) 40%, transparent);
    color: var(--color-warning, #f59e0b);
    font-size: var(--font-sm, 13px);
  }
  .ai-status-banner .ai-status-text {
    flex: 1;
  }
  .ai-status-banner .ai-status-icon {
    font-size: 1rem;
  }

  .mode-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .mode-header h2 {
    margin: 0;
    font-size: var(--font-md, 15px);
    font-weight: 600;
    color: var(--text-primary);
  }
  .hint {
    font-size: var(--font-xs, 11px);
    color: var(--text-muted);
  }

  .raw-textarea {
    width: 100%;
    min-height: 4.5rem;
    resize: vertical;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 6px);
    padding: 0.5rem;
    background: var(--surface-1);
    color: var(--text-primary);
    font-family: inherit;
    font-size: var(--font-body, 14px);
    line-height: 1.5;
    outline: none;
  }
  .raw-textarea:focus {
    border-color: var(--color-primary);
  }

  .sensitive-marks {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
    align-items: center;
  }
  .marks-label,
  .row-label,
  .section-label > span {
    font-size: var(--font-xs, 11px);
    color: var(--text-muted);
  }
  .mark {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    padding: 0.1rem 0.4rem;
    border-radius: var(--radius-sm, 4px);
    background: color-mix(in srgb, var(--color-danger, #ef4444) 12%, transparent);
    color: var(--color-danger, #ef4444);
    font-size: var(--font-xs, 11px);
  }
  .mark-remove {
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
    font-size: inherit;
    padding: 0;
    line-height: 1;
  }

  .enrich-status {
    font-size: var(--font-xs, 11px);
    color: var(--text-muted);
    padding: 0.25rem 0.4rem;
    border-radius: var(--radius-sm, 4px);
    background: var(--surface-2);
  }
  .enrich-status.merged,
  .enrich-status.partial {
    color: var(--color-primary);
  }
  .enrich-status.failed {
    color: var(--color-danger, #ef4444);
  }

  .preview {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.5rem;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 6px);
    background: var(--surface-1);
    overflow: auto;
  }

  .form-row {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .text-input,
  .kind-select,
  .notes-input {
    width: 100%;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm, 4px);
    padding: 0.3rem 0.4rem;
    background: var(--surface-2);
    color: var(--text-primary);
    font-family: inherit;
    font-size: var(--font-sm, 13px);
    outline: none;
  }
  .text-input:focus,
  .kind-select:focus,
  .notes-input:focus {
    border-color: var(--color-primary);
  }
  .notes-input {
    min-height: 2.5rem;
    resize: vertical;
  }

  .fields-section {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .section-label {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .field-row {
    display: flex;
    gap: 0.3rem;
    align-items: center;
  }
  .field-key {
    flex: 0 0 6rem;
  }
  .field-value {
    flex: 1;
    min-width: 0;
  }
  .sensitive-toggle {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    font-size: var(--font-xs, 11px);
    color: var(--text-muted);
    white-space: nowrap;
  }

  .tags-section {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .tag-list {
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    padding: 0.1rem 0.4rem;
    border-radius: var(--radius-sm, 4px);
    font-size: var(--font-xs, 11px);
  }
  .ai-tag {
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
    color: var(--color-primary);
  }
  .tag-remove {
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
    font-size: inherit;
    padding: 0;
    line-height: 1;
  }
  .tag-convert {
    background: none;
    border: none;
    color: inherit;
    opacity: 0.75;
    cursor: pointer;
    font-size: inherit;
    padding: 0;
    line-height: 1;
  }
  .tag-convert:hover {
    opacity: 1;
    text-decoration: underline;
  }

  .actions-row {
    display: flex;
    gap: 0.4rem;
    align-items: center;
    margin-top: auto;
  }

  .ghost-btn {
    padding: 0.25rem 0.55rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    color: var(--text-muted);
    border-radius: var(--radius-md, 6px);
    font-size: var(--font-xs, 11px);
    font-family: inherit;
    cursor: pointer;
  }
  .ghost-btn.small {
    padding: 0.15rem 0.35rem;
  }
  .ghost-btn.danger {
    color: var(--color-danger, #ef4444);
  }
  .ghost-btn:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--color-primary);
  }

  .primary-btn {
    margin-left: auto;
    padding: 0.35rem 0.85rem;
    background: var(--color-primary);
    border: 1px solid var(--color-primary);
    color: white;
    border-radius: var(--radius-md, 6px);
    font-size: var(--font-sm, 13px);
    font-weight: 500;
    font-family: inherit;
    cursor: pointer;
  }
  .primary-btn:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .error-banner {
    padding: 0.3rem 0.5rem;
    background: color-mix(in srgb, var(--color-danger, #ef4444) 12%, transparent);
    color: var(--color-danger, #ef4444);
    border-radius: var(--radius-sm, 4px);
    font-size: var(--font-xs, 11px);
  }

  .audit-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10;
  }
  .audit-dialog {
    width: min(90vw, 600px);
    max-height: 80vh;
    overflow: auto;
    background: var(--surface-0);
    border: 1px solid var(--border-emphasis);
    border-radius: var(--radius-md, 6px);
    padding: 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .audit-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .audit-header h3 {
    margin: 0;
    font-size: var(--font-md, 15px);
    color: var(--text-primary);
  }
  .audit-meta {
    display: flex;
    gap: 0.75rem;
    font-size: var(--font-xs, 11px);
    color: var(--text-muted);
  }
  .audit-messages {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .audit-msg {
    border-left: 2px solid var(--border-default);
    padding-left: 0.5rem;
  }
  .audit-msg strong {
    font-size: var(--font-xs, 11px);
    color: var(--text-muted);
    text-transform: uppercase;
    display: block;
    margin-bottom: 0.15rem;
  }
  .audit-msg pre {
    margin: 0;
    font-family: var(--font-family-mono, monospace);
    font-size: var(--font-xs, 11px);
    color: var(--text-primary);
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>
