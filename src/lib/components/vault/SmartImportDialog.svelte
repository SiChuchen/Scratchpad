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

    // URL with embedded credentials: scheme://user:pass@host
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

    // ssh user@host
    const m2 = text.match(/^ssh\s+([^\s@]+)@([^\s]+)$/)
    if (m2) {
      detected = 'credential'
      title = m2[2]
      fields = [{ key: 'user', value: m2[1], isSensitive: false }, { key: 'host', value: m2[2], isSensitive: false }]
      return
    }

    // user:pass@host:port
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

    // https://...
    const m4 = text.match(/^(https?:\/\/[^\s]+)$/i)
    if (m4) {
      detected = 'bookmark'
      title = m4[1]
      fields = [{ key: 'url', value: m4[1], isSensitive: false }]
      return
    }

    // fallback: nothing detected
    detected = null
  }

  async function save() {
    if (!detected || !title) return
    const input: VaultEntryInput = {
      kind: detected,
      title,
      fields,
      notes: null,
    }
    await vaultApi.createEntry(input)
    onImported?.()
    onClose?.()
  }
</script>

<div class="dialog">
  <h3>智能导入</h3>
  <textarea
    bind:value={raw}
    rows={4}
    placeholder="粘贴 ssh user@host、user:pass@host:port、https://... 等"
  ></textarea>
  <button onclick={parse}>解析</button>

  {#if detected}
    <div class="preview">
      <p>识别为：{detected === 'credential' ? '凭据' : '书签'}</p>
      <label>标题<input bind:value={title} /></label>
      {#each fields as f, i}
        <label>{f.key}
          <input bind:value={f.value} type={f.isSensitive ? 'password' : 'text'} />
        </label>
      {/each}
      <button onclick={save}>保存</button>
    </div>
  {/if}

  <button class="cancel" onclick={() => onClose?.()}>取消</button>
</div>

<style>
  .dialog { border: 1px solid var(--border-color, #ccc); padding: 12px; display: flex; flex-direction: column; gap: 8px; background: var(--bg, #fff); }
  textarea { width: 100%; }
  .preview { border-top: 1px dashed var(--border-color, #ccc); padding-top: 8px; display: flex; flex-direction: column; gap: 4px; }
  .preview label { display: flex; flex-direction: column; font-size: 0.85em; }
  .cancel { margin-top: 8px; }
</style>
