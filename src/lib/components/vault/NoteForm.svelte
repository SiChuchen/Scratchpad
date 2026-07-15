<script lang="ts">
  import { vaultApi } from '$lib/api/vault'
  import type { VaultEntry } from '$lib/types/vault'

  let { onSaved }: { onSaved?: (e: VaultEntry) => void } = $props()
  let title = $state('')
  let content = $state('')

  async function submit() {
    if (!title.trim()) return
    const entry = await vaultApi.createEntry({
      kind: 'note',
      title: title.trim(),
      fields: [],
      notes: content.trim() || null,
    })
    onSaved?.(entry)
    title = content = ''
  }
</script>

<form onsubmit={(e) => { e.preventDefault(); submit() }} class="vault-form">
  <label>标题<input bind:value={title} required /></label>
  <label>内容<textarea bind:value={content} rows={6}></textarea></label>
  <button type="submit">保存</button>
</form>

<style>
  .vault-form { display: flex; flex-direction: column; gap: 6px; }
  .vault-form label { display: flex; flex-direction: column; font-size: 0.85em; gap: 2px; }
  .vault-form input, .vault-form textarea { padding: 4px 6px; }
</style>
