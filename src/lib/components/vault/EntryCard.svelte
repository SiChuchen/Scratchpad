<script lang="ts">
  import TagEditor from './TagEditor.svelte'
  import { vaultApi } from '$lib/api/vault'
  import type { VaultEntryDetail } from '$lib/types/vault'

  let { entryId }: { entryId: string } = $props()

  let detail = $state<VaultEntryDetail | null>(null)
  let showSecret = $state<Record<string, boolean>>({})

  async function load() {
    detail = await vaultApi.getEntry(entryId)
  }

  async function copy(text: string) {
    // 用 webview 自带的 Clipboard API；ipc_clipboard_copy_file 是复制文件用的（CF_HDROP），不适用文本
    await navigator.clipboard.writeText(text).catch(() => {})
  }

  async function remove() {
    if (!confirm('确认删除？')) return
    await vaultApi.deleteEntry(entryId)
    window.dispatchEvent(new CustomEvent('vault-entry-deleted', { detail: entryId }))
  }

  // entryId 变化时重新加载。`void entryId` 显式标记依赖；
  // load() 写 detail 但不读 detail，不会循环触发
  $effect(() => {
    void entryId
    load()
  })
</script>

{#if detail}
  <div class="entry-card">
    <div class="entry-header">
      <strong>{detail.entry.title}</strong>
      <span class="kind-badge">{detail.entry.kind}</span>
      <button class="danger" onclick={remove}>删除</button>
    </div>

    <div class="fields">
      {#each detail.fields as f (f.key)}
        <div class="field">
          <span class="field-key">{f.key}</span>
          <code class="field-value">
            {#if f.isSensitive && !showSecret[f.key]}••••{:else}{f.value}{/if}
          </code>
          <div class="field-actions">
            {#if f.isSensitive}
              <button onclick={() => showSecret[f.key] = !showSecret[f.key]}>
                {showSecret[f.key] ? '隐藏' : '显示'}
              </button>
            {/if}
            <button onclick={() => copy(f.value)}>拷贝</button>
          </div>
        </div>
      {/each}
    </div>

    {#if detail.entry.notes}
      <div class="notes">{detail.entry.notes}</div>
    {/if}

    <TagEditor entryId={entryId} tags={detail.tags} />
  </div>
{/if}

<style>
  .entry-card { border: 1px solid var(--border-color, #ccc); border-radius: 6px; padding: 10px; display: flex; flex-direction: column; gap: 8px; }
  .entry-header { display: flex; gap: 8px; align-items: center; }
  .kind-badge { font-size: 0.7em; opacity: 0.6; padding: 2px 6px; background: rgba(0,0,0,0.06); border-radius: 4px; }
  .fields { display: flex; flex-direction: column; gap: 4px; }
  .field { display: flex; gap: 8px; align-items: center; }
  .field-key { width: 80px; opacity: 0.7; font-size: 0.85em; }
  .field-value { flex: 1; }
  .field-actions { display: flex; gap: 4px; }
  .field-actions button { font-size: 0.75em; padding: 2px 6px; }
  .notes { font-size: 0.85em; opacity: 0.7; white-space: pre-wrap; }
  .danger { color: #c33; margin-left: auto; }
</style>
