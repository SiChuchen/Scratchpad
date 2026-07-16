<script lang="ts">
  import { onMount } from 'svelte'
  import { vaultApi } from '$lib/api/vault'
  import { onTagsUpdated, onLlmError } from '$lib/api/vault'
  import type { EntryKind, VaultEntry } from '$lib/types/vault'
  import CredentialForm from '$lib/components/vault/CredentialForm.svelte'
  import BookmarkForm from '$lib/components/vault/BookmarkForm.svelte'
  import NoteForm from '$lib/components/vault/NoteForm.svelte'
  import EntryCard from '$lib/components/vault/EntryCard.svelte'
  import SearchBar from '$lib/components/vault/SearchBar.svelte'
  import SmartImportDialog from '$lib/components/vault/SmartImportDialog.svelte'
  import LlmSearchPanel from '$lib/components/vault/LlmSearchPanel.svelte'
  import type { VaultSearchHit } from '$lib/types/vault'

  type Filter = EntryKind | 'all' | 'search'
  let activeFilter = $state<Filter>('all')
  let entries = $state<VaultEntry[]>([])
  let loading = $state(false)
  let showForm = $state<null | 'credential' | 'bookmark' | 'note'>(null)
  let showImport = $state(false)
  let searchResults = $state<VaultSearchHit[] | null>(null)

  function handleResults(hits: VaultSearchHit[]) {
    searchResults = hits
  }

  function handleClear() {
    searchResults = null
  }

  async function reload() {
    if (activeFilter === 'search') return
    loading = true
    try {
      const kind = activeFilter === 'all' ? undefined : activeFilter
      entries = await vaultApi.listEntries(kind)
    } finally {
      loading = false
    }
  }

  function switchFilter(f: Filter) {
    activeFilter = f
    searchResults = null
    if (f !== 'search') reload()
  }

  onMount(() => {
    reload()
    onTagsUpdated(() => reload())
    onLlmError((e) => alert(`LLM 错误: ${e.kind} - ${e.message}`))
  })
</script>

<div class="vault-view">
  <div class="vault-header">
    <div class="vault-filters">
      <button class="filter-btn" class:active={activeFilter === 'all'} onclick={() => switchFilter('all')}>全部</button>
      <button class="filter-btn" class:active={activeFilter === 'credential'} onclick={() => switchFilter('credential')}>凭据</button>
      <button class="filter-btn" class:active={activeFilter === 'bookmark'} onclick={() => switchFilter('bookmark')}>书签</button>
      <button class="filter-btn" class:active={activeFilter === 'note'} onclick={() => switchFilter('note')}>安全笔记</button>
      <button class="filter-btn ai-filter" class:active={activeFilter === 'search'} onclick={() => switchFilter('search')} title="LLM 自然语言搜索">🤖 智能搜索</button>
    </div>
    {#if activeFilter !== 'search'}
      <SearchBar onResults={handleResults} onClear={handleClear} />
    {/if}
    <div class="vault-actions">
      <button class="action-btn" onclick={() => showForm = 'credential'} title="新建凭据">+ 凭据</button>
      <button class="action-btn" onclick={() => showForm = 'bookmark'} title="新建书签">+ 书签</button>
      <button class="action-btn" onclick={() => showForm = 'note'} title="新建笔记">+ 笔记</button>
      <button class="action-btn" onclick={() => showImport = true} title="智能导入">📥</button>
    </div>
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
    </div>
  {/if}

  {#if showImport}
    <SmartImportDialog onClose={() => showImport = false} onImported={() => { showImport = false; reload() }} />
  {/if}

  {#if activeFilter === 'search'}
    <LlmSearchPanel />
  {:else}
    <div class="vault-body">
      {#if searchResults}
        {#if searchResults.length === 0}
          <div class="dock-empty">
            <div>未找到匹配条目</div>
            <div class="hint">尝试其它关键词，或清空搜索框查看全部</div>
          </div>
        {:else}
          <div class="entry-list">
            {#each searchResults as hit (hit.entry.id)}
              <EntryCard entryId={hit.entry.id} />
            {/each}
          </div>
        {/if}
      {:else if loading}
        <div class="dock-empty">
          <div>加载中...</div>
        </div>
      {:else if entries.length === 0}
        <div class="dock-empty">
          <div>暂无条目</div>
          <div class="hint">点击右上方「+ 凭据 / + 书签 / + 笔记」按钮添加</div>
        </div>
      {:else}
        <div class="entry-list">
          {#each entries as e (e.id)}
            <EntryCard entryId={e.id} />
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .vault-view {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.5rem 0.65rem;
    overflow: hidden;
    min-height: 0;
  }

  .vault-header {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    flex-shrink: 0;
    min-height: 1.4rem;
  }

  .vault-filters {
    display: flex;
    gap: 0.15rem;
    flex-shrink: 0;
  }

  .filter-btn {
    background: none;
    border: 1px solid transparent;
    color: color-mix(in srgb, var(--text-primary) 50%, transparent);
    font-size: var(--font-sm, 0.65rem);
    font-weight: 500;
    padding: 0.2rem 0.45rem;
    border-radius: var(--radius-md, 0.3rem);
    cursor: pointer;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
    font-family: inherit;
    white-space: nowrap;
  }

  .filter-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
    border-color: color-mix(in srgb, var(--text-primary) 15%, transparent);
  }

  .filter-btn.active {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 15%, transparent);
    border-color: color-mix(in srgb, var(--text-primary) 25%, transparent);
  }

  .ai-filter.active {
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border-color: color-mix(in srgb, var(--color-primary) 30%, transparent);
  }

  .vault-actions {
    display: flex;
    gap: 0.15rem;
    flex-shrink: 0;
  }

  .action-btn {
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    color: var(--text-muted);
    font-size: var(--font-sm, 0.65rem);
    padding: 0.2rem 0.45rem;
    border-radius: var(--radius-md, 0.3rem);
    cursor: pointer;
    transition: background 0.12s, color 0.12s, border-color 0.12s;
    font-family: inherit;
    white-space: nowrap;
  }

  .action-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border-color: color-mix(in srgb, var(--color-primary) 30%, transparent);
    color: var(--color-primary);
  }

  .form-panel {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.5rem;
    background: var(--surface-1);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg, 0.5rem);
    flex-shrink: 0;
  }

  .vault-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .dock-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 2rem 0.5rem;
    color: var(--text-faint);
    font-size: var(--font-sm, 0.75rem);
    text-align: center;
    gap: 0.2rem;
  }

  .dock-empty .hint {
    font-size: var(--font-sm, 0.65rem);
    color: var(--text-faint);
    opacity: 0.7;
  }

  .entry-list {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
</style>
