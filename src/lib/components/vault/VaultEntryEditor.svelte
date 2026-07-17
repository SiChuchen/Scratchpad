<script lang="ts">
  // src/lib/components/vault/VaultEntryEditor.svelte
  //
  // 创建 / 编辑共用表单。支持：
  //   * kind 切换（不同 kind 默认 fields 不同）；
  //   * title / notes；
  //   * 动态 field 列表（key / value / sensitive 眼睛切换 / 删除）；
  //   * manual tags 文本输入（逗号分隔）；
  //   * 编辑模式下 AI tags 只读展示，每个带移除按钮调 onRemoveAiTag。
  //
  // 绝不使用 confirm() / alert()。

  import { untrack } from 'svelte'
  import type {
    EntryKind,
    FieldInput,
    VaultEntryDetail,
    VaultEntryInput,
    VaultTag,
  } from '$lib/types/vault'
  import { messages, isZh } from '$lib/i18n'

  interface Props {
    mode: 'create' | 'edit'
    initial?: VaultEntryDetail
    initialKind?: EntryKind
    onSave: (input: VaultEntryInput) => Promise<void>
    onCancel: () => void
    onRemoveAiTag?: (id: string, normalizedTag: string) => Promise<void>
  }

  let {
    mode,
    initial,
    initialKind = 'credential',
    onSave,
    onCancel,
    onRemoveAiTag,
  }: Props = $props()

  interface FieldRow {
    id: string
    key: string
    value: string
    isSensitive: boolean
    reveal: boolean
  }

  function defaultFieldsFor(kind: EntryKind): FieldRow[] {
    if (kind === 'credential') {
      return [
        mkRow('user', '', false),
        mkRow('password', '', true),
        mkRow('host', '', false),
      ]
    }
    if (kind === 'bookmark') {
      return [mkRow('url', '', false)]
    }
    return []
  }

  function mkRow(key: string, value: string, isSensitive: boolean): FieldRow {
    return {
      id: `${Math.random().toString(36).slice(2, 9)}`,
      key,
      value,
      isSensitive,
      reveal: false,
    }
  }

  const initialState = untrack(() => {
    const detail = initial
    const kind = detail?.entry.kind ?? initialKind
    return {
      kind,
      title: detail?.entry.title ?? '',
      notes: detail?.entry.notes ?? '',
      fields: detail
        ? detail.fields
          .slice()
          .sort((a, b) => a.sortOrder - b.sortOrder)
          .map((f) => ({
            id: f.id,
            key: f.key,
            value: f.value,
            isSensitive: f.isSensitive,
            reveal: false,
          }))
        : defaultFieldsFor(kind),
      manualTagsInput: detail
        ? detail.tags
          .filter((t) => t.source === 'manual')
          .map((t) => t.tag)
          .join(', ')
        : '',
    }
  })

  let kind = $state<EntryKind>(initialState.kind)
  let title = $state(initialState.title)
  let notes = $state(initialState.notes)
  let fields = $state<FieldRow[]>(initialState.fields)
  let manualTagsInput = $state(initialState.manualTagsInput)

  let saving = $state(false)

  // Field row labels (used for aria-labels, distinct from display labels).
  // zh-CN: 字段名 / 字段值; en: Field name / Field value. These are not in
  // LocaleMessages because the library block uses generic 键/值 placeholders
  // for the capture preview; here we want slightly more descriptive names.
  const fieldKeyLabel = $derived(isZh() ? '字段名' : 'Field name')
  const fieldValueLabel = $derived(isZh() ? '字段值' : 'Field value')
  const tagsLabel = $derived(isZh() ? '标签（逗号分隔）' : 'Tags (comma separated)')

  // kind 切换（仅 create 模式有效）→ 替换 fields
  function onKindChange(next: EntryKind) {
    if (mode === 'edit') return
    kind = next
    fields = defaultFieldsFor(next)
  }

  function addField() {
    fields = [...fields, mkRow('', '', false)]
  }

  function removeField(id: string) {
    fields = fields.filter((f) => f.id !== id)
  }

  function toggleReveal(id: string) {
    fields = fields.map((f) =>
      f.id === id ? { ...f, reveal: !f.reveal } : f,
    )
  }

  function toggleSensitive(id: string) {
    fields = fields.map((f) =>
      f.id === id ? { ...f, isSensitive: !f.isSensitive, reveal: false } : f,
    )
  }

  function parseManualTags(text: string): string[] {
    return text
      .split(',')
      .map((s) => s.trim())
      .filter((s) => s.length > 0)
  }

  // AI tags：编辑模式从 initial 取出 source === 'ai' 的，按 normalizedTag 去重
  const aiTags = $derived.by<VaultTag[]>(() => {
    if (!initial) return []
    const seen = new Set<string>()
    const out: VaultTag[] = []
    for (const t of initial.tags) {
      if (t.source !== 'ai') continue
      if (seen.has(t.normalizedTag)) continue
      seen.add(t.normalizedTag)
      out.push(t)
    }
    return out
  })

  async function handleRemoveAiTag(normalizedTag: string) {
    if (!initial || !onRemoveAiTag) return
    await onRemoveAiTag(initial.entry.id, normalizedTag)
  }

  async function handleSubmit(e: SubmitEvent) {
    e.preventDefault()
    if (saving) return
    saving = true
    try {
      const input: VaultEntryInput = {
        kind,
        title: title.trim(),
        fields: fields
          .filter((f) => f.key.trim().length > 0 || f.value.length > 0)
          .map<FieldInput>((f) => ({
            key: f.key.trim(),
            value: f.value,
            isSensitive: f.isSensitive,
          })),
        notes: notes.trim().length > 0 ? notes.trim() : null,
        manualTags: parseManualTags(manualTagsInput),
      }
      await onSave(input)
    } finally {
      saving = false
    }
  }
</script>

<form onsubmit={handleSubmit} class="editor">
  <div class="field">
    <span class="label">{messages.library.kind}</span>
    <div class="kind-row" role="radiogroup" aria-label={messages.library.kind}>
      {#each ['credential', 'bookmark', 'note'] as k (k)}
        <button
          type="button"
          class="kind-btn"
          class:active={kind === k}
          aria-pressed={kind === k}
          disabled={mode === 'edit'}
          onclick={() => onKindChange(k as EntryKind)}
        >
          {k === 'credential' ? messages.library.credential : k === 'bookmark' ? messages.library.bookmark : messages.library.note}
        </button>
      {/each}
    </div>
  </div>

  <label class="field">
    <span class="label">{messages.library.titleLabel}</span>
    <input class="input" bind:value={title} placeholder={messages.library.titlePlaceholder} required />
  </label>

  <div class="fields-list">
    <div class="fields-header">
      <span class="label">{messages.library.fieldsLabel}</span>
      <button type="button" class="add-btn" onclick={addField}>+ {messages.library.addField}</button>
    </div>
    {#each fields as f (f.id)}
      <div class="field-row">
        <input
          class="input field-key"
          placeholder={fieldKeyLabel}
          bind:value={f.key}
          aria-label={fieldKeyLabel}
        />
        <input
          class="input field-value"
          type={f.isSensitive && !f.reveal ? 'password' : 'text'}
          placeholder={fieldValueLabel}
          bind:value={f.value}
          aria-label={fieldValueLabel}
        />
        <button
          type="button"
          class="icon-btn"
          aria-label={f.isSensitive ? messages.quickAccess.removeSensitiveMark : messages.quickAccess.markSensitive}
          title={f.isSensitive ? messages.quickAccess.removeSensitiveMark : messages.quickAccess.markSensitive}
          onclick={() => toggleSensitive(f.id)}
        >
          {#if f.isSensitive}
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
              <rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect>
              <path d="M7 11V7a5 5 0 0 1 10 0v4"></path>
            </svg>
          {:else}
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
              <rect x="3" y="11" width="18" height="11" rx="2" ry="2"></rect>
              <path d="M7 11V7a5 5 0 0 1 9.9-1"></path>
            </svg>
          {/if}
        </button>
        {#if f.isSensitive}
          <button
            type="button"
            class="icon-btn"
            aria-label={f.reveal ? messages.library.hideLabel.replace('{label}', fieldValueLabel) : messages.library.showLabel.replace('{label}', fieldValueLabel)}
            title={f.reveal ? messages.library.hideLabel.replace('{label}', fieldValueLabel) : messages.library.showLabel.replace('{label}', fieldValueLabel)}
            onclick={() => toggleReveal(f.id)}
          >
            {#if f.reveal}
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
                <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"></path>
                <line x1="1" y1="1" x2="23" y2="23"></line>
              </svg>
            {:else}
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
                <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"></path>
                <circle cx="12" cy="12" r="3"></circle>
              </svg>
            {/if}
          </button>
        {/if}
        <button
          type="button"
          class="icon-btn danger"
          aria-label={messages.library.delete}
          title={messages.library.delete}
          onclick={() => removeField(f.id)}
        >
          ×
        </button>
      </div>
    {/each}
  </div>

  <label class="field">
    <span class="label">{messages.library.notesLabel}</span>
    <textarea class="input textarea" bind:value={notes} rows={2}></textarea>
  </label>

  <label class="field">
    <span class="label">{tagsLabel}</span>
    <input class="input" bind:value={manualTagsInput} placeholder={messages.library.tagsPlaceholder} />
  </label>

  {#if mode === 'edit' && aiTags.length > 0}
    <div class="ai-tags">
      <span class="label">{messages.library.aiTag}</span>
      <div class="ai-tag-list">
        {#each aiTags as t (t.normalizedTag)}
          <span class="ai-tag-chip">
            {t.tag}
            {#if onRemoveAiTag}
              <button
                type="button"
                class="ai-tag-remove"
                aria-label={`${messages.library.delete} ${t.tag}`}
                onclick={() => handleRemoveAiTag(t.normalizedTag)}
              >×</button>
            {/if}
          </span>
        {/each}
      </div>
    </div>
  {/if}

  <div class="actions">
    <button type="button" class="btn-secondary" onclick={onCancel} disabled={saving}>{messages.home.cancel}</button>
    <button type="submit" class="btn-primary" disabled={saving}>
      {saving ? messages.quickAccess.saving : messages.quickAccess.save}
    </button>
  </div>
</form>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .label {
    font-size: var(--font-sm, 0.6rem);
    color: var(--text-muted);
    font-weight: 500;
  }

  .input {
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 0.3rem);
    color: var(--text-primary);
    font-size: var(--font-sm, 0.7rem);
    font-family: inherit;
    padding: 0.3rem 0.45rem;
    outline: none;
    transition: border-color 0.12s;
    width: 100%;
  }

  .input:focus {
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
  }

  .textarea {
    resize: vertical;
    min-height: 2rem;
    line-height: 1.45;
  }

  .kind-row {
    display: flex;
    gap: 0.3rem;
  }

  .kind-btn {
    flex: 1;
    padding: 0.3rem 0.4rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 0.25rem);
    color: var(--text-muted);
    font-size: var(--font-sm, 0.65rem);
    cursor: pointer;
    font-family: inherit;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
  }

  .kind-btn.active {
    color: var(--color-primary);
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
  }

  .kind-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .fields-list {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .fields-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .add-btn {
    background: none;
    border: none;
    color: var(--color-primary);
    font-size: var(--font-sm, 0.6rem);
    cursor: pointer;
    padding: 0;
    font-family: inherit;
  }

  .add-btn:hover {
    text-decoration: underline;
  }

  .field-row {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }

  .field-key {
    flex: 0 0 5rem;
  }

  .field-value {
    flex: 1;
    min-width: 0;
  }

  .icon-btn {
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    color: var(--text-muted);
    padding: 0.25rem 0.35rem;
    border-radius: var(--radius-md, 0.25rem);
    cursor: pointer;
    font-family: inherit;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
  }

  .icon-btn:hover {
    color: var(--color-primary);
    border-color: color-mix(in srgb, var(--color-primary) 30%, transparent);
  }

  .icon-btn.danger:hover {
    color: #ff6b6b;
    border-color: color-mix(in srgb, #ff6b6b 30%, transparent);
    background: color-mix(in srgb, #ff6b6b 10%, transparent);
  }

  .ai-tags {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    padding: 0.3rem;
    background: color-mix(in srgb, var(--color-primary) 6%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 20%, transparent);
    border-radius: var(--radius-md, 0.3rem);
  }

  .ai-tag-list {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
  }

  .ai-tag-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    font-size: 0.6rem;
    padding: 0.1rem 0.35rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 0.2rem);
    color: var(--text-primary);
  }

  .ai-tag-remove {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 0.7rem;
    line-height: 1;
    padding: 0;
    font-family: inherit;
  }

  .ai-tag-remove:hover {
    color: #ff6b6b;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.3rem;
    margin-top: 0.2rem;
  }

  .btn-primary,
  .btn-secondary {
    padding: 0.3rem 0.7rem;
    border-radius: var(--radius-md, 0.3rem);
    font-size: var(--font-sm, 0.65rem);
    font-weight: 500;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s, border-color 0.12s;
  }

  .btn-primary {
    background: color-mix(in srgb, var(--color-primary) 18%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 35%, transparent);
    color: var(--color-primary);
  }

  .btn-primary:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-primary) 28%, transparent);
  }

  .btn-secondary {
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    color: var(--text-muted);
  }

  .btn-secondary:hover:not(:disabled) {
    color: var(--text-primary);
  }

  .btn-primary:disabled,
  .btn-secondary:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }
</style>
