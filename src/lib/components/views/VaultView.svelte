<script lang="ts">
  import { onMount } from 'svelte'
  import { vaultApi } from '$lib/api/vault'
  import type { EntryKind, VaultEntry } from '$lib/types/vault'
  import CredentialForm from '$lib/components/vault/CredentialForm.svelte'
  import BookmarkForm from '$lib/components/vault/BookmarkForm.svelte'
  import NoteForm from '$lib/components/vault/NoteForm.svelte'

  let activeKind = $state<EntryKind | 'all'>('all')
  let entries = $state<VaultEntry[]>([])
  let loading = $state(false)
  let showForm = $state<null | 'credential' | 'bookmark' | 'note'>(null)

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
    <span style="flex:1"></span>
    <button onclick={() => showForm = 'credential'}>+ 凭据</button>
    <button onclick={() => showForm = 'bookmark'}>+ 书签</button>
    <button onclick={() => showForm = 'note'}>+ 笔记</button>
  </div>

  {#if showForm}
    <div class="form-panel">
      {#if showForm === 'credential'}
        <CredentialForm onSaved={() => { showForm = null; reload() }} />
      {:else if showForm === 'bookmark'}
        <BookmarkForm onSaved={() => { showForm = null; reload() }} />
      {:else if showForm === 'note'}
        <NoteForm onSaved={() => { showForm = null; reload() }} />
      {/if}
      <button onclick={() => showForm = null}>取消</button>
    </div>
  {/if}

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
  .form-panel { display: flex; flex-direction: column; gap: 6px; padding: 8px; border: 1px solid #ddd; border-radius: 4px; background: rgba(0,0,0,0.02); }
  .form-panel button { align-self: flex-start; padding: 4px 12px; cursor: pointer; }
</style>
