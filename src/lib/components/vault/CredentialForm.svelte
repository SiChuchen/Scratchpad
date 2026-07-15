<script lang="ts">
  import { vaultApi } from '$lib/api/vault'
  import type { VaultEntry } from '$lib/types/vault'

  let { onSaved }: { onSaved?: (e: VaultEntry) => void } = $props()

  let title = $state('')
  let username = $state('')
  let password = $state('')
  let host = $state('')
  let notes = $state('')

  async function submit() {
    if (!title.trim()) return
    const entry = await vaultApi.createEntry({
      kind: 'credential',
      title: title.trim(),
      fields: [
        { key: 'user', value: username, isSensitive: false },
        { key: 'password', value: password, isSensitive: true },
        { key: 'host', value: host, isSensitive: false },
      ].filter(f => f.value.length > 0),
      notes: notes.trim() || null,
    })
    onSaved?.(entry)
    title = username = password = host = notes = ''
  }
</script>

<form onsubmit={(e) => { e.preventDefault(); submit() }} class="vault-form">
  <label>标题<input bind:value={title} placeholder="例如：生产数据库" required /></label>
  <label>用户名<input bind:value={username} placeholder="user" /></label>
  <label>密码<input type="password" bind:value={password} placeholder="••••" /></label>
  <label>主机/URL<input bind:value={host} placeholder="10.0.0.1 或 https://..." /></label>
  <label>备注<textarea bind:value={notes} rows={2}></textarea></label>
  <button type="submit">保存</button>
</form>

<style>
  .vault-form { display: flex; flex-direction: column; gap: 6px; }
  .vault-form label { display: flex; flex-direction: column; font-size: 0.85em; gap: 2px; }
  .vault-form input, .vault-form textarea { padding: 4px 6px; }
</style>
