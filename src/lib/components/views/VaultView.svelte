<script lang="ts">
  import { onMount } from 'svelte'
  import { vaultApi } from '$lib/api/vault'
  import type { EntryKind, VaultEntry } from '$lib/types/vault'

  let activeKind = $state<EntryKind | 'all'>('all')
  let entries = $state<VaultEntry[]>([])
  let loading = $state(false)

  async function reload() {
    loading = true
    try {
      entries = await vaultApi.listEntries(activeKind === 'all' ? undefined : activeKind)
    } finally {
      loading = false
    }
  }

  onMount(reload)
</script>

<div class="vault-view">
  <div class="vault-tabs">
    <button class:active={activeKind === 'all'} onclick={() => { activeKind = 'all'; reload() }}>全部</button>
    <button class:active={activeKind === 'credential'} onclick={() => { activeKind = 'credential'; reload() }}>凭据</button>
    <button class:active={activeKind === 'bookmark'} onclick={() => { activeKind = 'bookmark'; reload() }}>书签</button>
    <button class:active={activeKind === 'note'} onclick={() => { activeKind = 'note'; reload() }}>安全笔记</button>
  </div>

  <div class="vault-content">
    {#if loading}
      <div>加载中...</div>
    {:else if entries.length === 0}
      <div>暂无条目，点击「+ 新建」添加</div>
    {:else}
      <ul>
        {#each entries as e (e.id)}
          <li>{e.title} <span class="kind-badge">{e.kind}</span></li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .vault-view { padding: 12px; display: flex; flex-direction: column; gap: 8px; }
  .vault-tabs { display: flex; gap: 4px; }
  .vault-tabs button { padding: 4px 12px; cursor: pointer; }
  .vault-tabs button.active { background: var(--accent-color, #4a9); color: white; }
  .kind-badge { font-size: 0.75em; opacity: 0.6; margin-left: 6px; }
</style>
