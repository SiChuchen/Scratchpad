<script lang="ts">
  import { vaultApi } from '$lib/api/vault'
  import type { VaultEntry } from '$lib/types/vault'

  let { onSaved }: { onSaved?: (e: VaultEntry) => void } = $props()
  let title = $state('')
  let url = $state('')
  let notes = $state('')

  async function submit() {
    if (!title.trim()) return
    const entry = await vaultApi.createEntry({
      kind: 'bookmark',
      title: title.trim(),
      fields: [{ key: 'url', value: url, isSensitive: false }].filter(f => f.value.length > 0),
      notes: notes.trim() || null,
    })
    onSaved?.(entry)
    title = url = notes = ''
  }
</script>

<form onsubmit={(e) => { e.preventDefault(); submit() }} class="vault-form">
  <label>标题<input bind:value={title} placeholder="例如：公司 Wiki" required /></label>
  <label>URL<input bind:value={url} placeholder="https://..." /></label>
  <label>备注<textarea bind:value={notes} rows={2}></textarea></label>
  <button type="submit">保存</button>
</form>

<style>
  .vault-form { display: flex; flex-direction: column; gap: 6px; }
  .vault-form label { display: flex; flex-direction: column; font-size: 0.85em; gap: 2px; }
  .vault-form input, .vault-form textarea { padding: 4px 6px; }
</style>
