<script lang="ts">
  import { vaultApi } from '$lib/api/vault'
  import type { FieldInput, VaultEntryInput } from '$lib/types/vault'

  let { onClose, onImported }: { onClose?: () => void; onImported?: () => void } = $props()

  let raw = $state('')
  let detected = $state<'credential' | 'bookmark' | null>(null)
  let fields = $state<FieldInput[]>([])
  let title = $state('')

  function parse() {
    fields = []
    detected = null
    title = ''
    const text = raw.trim()
    if (!text) return

    const m1 = text.match(/^([a-zA-Z][a-zA-Z0-9+.-]*):\/\/([^:/@\s]+):([^:/@\s]+)@([^\s/]+)/)
    if (m1) {
      detected = 'credential'
      title = m1[4]
      fields = [
        { key: 'user', value: m1[2], isSensitive: false },
        { key: 'password', value: m1[3], isSensitive: true },
        { key: 'host', value: `${m1[1]}://${m1[4]}`, isSensitive: false },
      ]
      return
    }

    const m2 = text.match(/^ssh\s+([^\s@]+)@([^\s]+)$/)
    if (m2) {
      detected = 'credential'
      title = m2[2]
      fields = [{ key: 'user', value: m2[1], isSensitive: false }, { key: 'host', value: m2[2], isSensitive: false }]
      return
    }

    const m3 = text.match(/^([^\s:]+):([^\s@]+)@([^\s:]+):?(\d*)$/)
    if (m3) {
      detected = 'credential'
      title = m3[3]
      fields = [
        { key: 'user', value: m3[1], isSensitive: false },
        { key: 'password', value: m3[2], isSensitive: true },
        { key: 'host', value: m3[4] ? `${m3[3]}:${m3[4]}` : m3[3], isSensitive: false },
      ]
      return
    }

    const m4 = text.match(/^(https?:\/\/[^\s]+)$/i)
    if (m4) {
      detected = 'bookmark'
      title = m4[1]
      fields = [{ key: 'url', value: m4[1], isSensitive: false }]
      return
    }

    detected = null
  }

  async function save() {
    if (!detected || !title) return
    const input: VaultEntryInput = {
      kind: detected,
      title,
      fields,
      notes: null,
      manualTags: [],
    }
    await vaultApi.createEntry(input)
    onImported?.()
    onClose?.()
  }
</script>

<div class="dialog">
  <div class="dialog-header">
    <span class="dialog-title">📥 智能导入</span>
    <button class="close-btn" onclick={() => onClose?.()} title="关闭" aria-label="关闭">✕</button>
  </div>

  <div class="section">
    <div class="label">粘贴原始文本</div>
    <textarea
      class="raw-input"
      bind:value={raw}
      rows={3}
      placeholder="ssh user@host、user:pass@host:port、https://... 等"
    ></textarea>
    <div class="row">
      <button class="btn-secondary" onclick={parse}>解析</button>
    </div>
  </div>

  {#if detected}
    <div class="preview">
      <div class="detected">
        识别为：<span class="kind">{detected === 'credential' ? '凭据' : '书签'}</span>
      </div>
      <label class="field">
        <span class="label">标题</span>
        <input class="input" bind:value={title} />
      </label>
      {#each fields as f}
        <label class="field">
          <span class="label">{f.key}</span>
          <input class="input" bind:value={f.value} type={f.isSensitive ? 'password' : 'text'} />
        </label>
      {/each}
      <div class="actions">
        <button class="btn-submit" onclick={save}>保存</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .dialog {
    background: var(--surface-1);
    border: 1px solid var(--border-emphasis);
    border-radius: var(--radius-lg, 0.5rem);
    padding: 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    box-shadow: var(--shadow-default);
  }

  .dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .dialog-title {
    font-size: var(--font-sm, 0.75rem);
    font-weight: 600;
    color: var(--text-primary);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 0.8rem;
    padding: 0.1rem 0.3rem;
    border-radius: var(--radius-md, 0.25rem);
    transition: color 0.12s, background 0.12s;
  }

  .close-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .label {
    font-size: var(--font-sm, 0.6rem);
    color: var(--text-muted);
    font-weight: 500;
  }

  .raw-input {
    width: 100%;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 0.3rem);
    color: var(--text-primary);
    font-size: var(--font-sm, 0.7rem);
    font-family: var(--font-family-mono, "Cascadia Code", "Consolas", monospace);
    padding: 0.35rem 0.45rem;
    outline: none;
    resize: vertical;
    line-height: 1.45;
    transition: border-color 0.12s;
  }

  .raw-input::placeholder {
    color: var(--text-faint);
  }

  .raw-input:focus {
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
  }

  .row {
    display: flex;
    gap: 0.25rem;
  }

  .btn-secondary {
    padding: 0.25rem 0.7rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    color: var(--text-muted);
    font-size: var(--font-sm, 0.65rem);
    border-radius: var(--radius-md, 0.3rem);
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s, color 0.12s;
  }

  .btn-secondary:hover {
    background: var(--border-default);
    color: var(--text-primary);
  }

  .preview {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding-top: 0.4rem;
    border-top: 1px solid var(--border-subtle);
  }

  .detected {
    font-size: var(--font-sm, 0.65rem);
    color: var(--text-muted);
  }

  .kind {
    color: var(--color-primary);
    font-weight: 600;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
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

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.25rem;
    margin-top: 0.1rem;
  }

  .btn-submit {
    padding: 0.25rem 0.7rem;
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    color: var(--color-primary);
    font-size: var(--font-sm, 0.65rem);
    font-weight: 500;
    border-radius: var(--radius-md, 0.3rem);
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s;
  }

  .btn-submit:hover {
    background: color-mix(in srgb, var(--color-primary) 25%, transparent);
  }
</style>
