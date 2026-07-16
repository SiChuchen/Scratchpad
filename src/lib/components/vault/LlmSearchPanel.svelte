<script lang="ts">
  import { vaultApi } from '$lib/api/vault'
  import type { VaultSearchHit } from '$lib/types/vault'
  import EntryCard from './EntryCard.svelte'

  let query = $state('')
  let loading = $state(false)
  let results = $state<VaultSearchHit[] | null>(null)
  let error = $state<string | null>(null)

  async function search() {
    const text = query.trim()
    if (!text) return
    loading = true
    error = null
    results = null
    try {
      results = await vaultApi.llmSearch(text, 20)
    } catch (e: any) {
      error = typeof e === 'string' ? e : (e?.message ?? String(e))
    } finally {
      loading = false
    }
  }

  function clear() {
    query = ''
    results = null
    error = null
  }
</script>

<div class="llm-search">
  <div class="header">
    <div class="title">🤖 智能搜索</div>
    <div class="subtitle">用自然语言描述你要找的条目，LLM 会脱敏后在你的 Vault 里匹配</div>
  </div>

  <div class="query-box">
    <textarea
      class="query-input"
      bind:value={query}
      rows={3}
      placeholder="例如：上个月加的那个生产数据库    或者    github 上我存的那个 token"
      onkeydown={(e) => {
        if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') search()
      }}
    ></textarea>
    <div class="query-actions">
      <span class="hint">⌘/Ctrl + Enter 提交</span>
      <div class="spacer"></div>
      {#if query}
        <button class="btn-secondary" onclick={clear}>清空</button>
      {/if}
      <button class="btn-submit" onclick={search} disabled={loading || !query.trim()}>
        {loading ? '搜索中...' : '搜索'}
      </button>
    </div>
  </div>

  {#if error}
    <div class="error">⚠ {error}</div>
  {/if}

  {#if results !== null}
    <div class="results">
      {#if results.length === 0}
        <div class="dock-empty">
          <div>未找到匹配条目</div>
          <div class="hint">尝试换个描述，或更具体的特征</div>
        </div>
      {:else}
        <div class="result-meta">找到 {results.length} 条匹配</div>
        <div class="entry-list">
          {#each results as hit (hit.summary.entry.id)}
            <EntryCard
              summary={hit.summary}
              onCopy={(p) => navigator.clipboard.writeText(p.value).catch(() => {})}
              onLoadDetail={(id) => vaultApi.getEntry(id)}
              onEdit={(id) => {}}
              onDelete={(id) => { void vaultApi.deleteEntry(id) }}
            />
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .llm-search {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    overflow: hidden;
    min-height: 0;
  }

  .header {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    flex-shrink: 0;
  }

  .title {
    font-size: var(--font-sm, 0.75rem);
    font-weight: 600;
    color: var(--text-primary);
  }

  .subtitle {
    font-size: var(--font-sm, 0.6rem);
    color: var(--text-muted);
    line-height: 1.4;
  }

  .query-box {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    flex-shrink: 0;
  }

  .query-input {
    width: 100%;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 0.3rem);
    color: var(--text-primary);
    font-size: var(--font-sm, 0.7rem);
    font-family: inherit;
    padding: 0.4rem 0.5rem;
    outline: none;
    resize: vertical;
    min-height: 2.6rem;
    line-height: 1.45;
    transition: border-color 0.12s;
  }

  .query-input::placeholder {
    color: var(--text-faint);
  }

  .query-input:focus {
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
  }

  .query-actions {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  .hint {
    font-size: 0.55rem;
    color: var(--text-faint);
    opacity: 0.7;
  }

  .spacer {
    flex: 1;
  }

  .btn-secondary {
    padding: 0.25rem 0.6rem;
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

  .btn-submit {
    padding: 0.25rem 0.8rem;
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    color: var(--color-primary);
    font-size: var(--font-sm, 0.65rem);
    font-weight: 500;
    border-radius: var(--radius-md, 0.3rem);
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s, opacity 0.12s;
  }

  .btn-submit:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-primary) 25%, transparent);
  }

  .btn-submit:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .error {
    background: color-mix(in srgb, #ff6b6b 12%, transparent);
    color: #ff6b6b;
    border-radius: var(--radius-md, 0.3rem);
    padding: 0.3rem 0.5rem;
    font-size: var(--font-sm, 0.65rem);
    flex-shrink: 0;
  }

  .results {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .result-meta {
    font-size: 0.6rem;
    color: var(--text-faint);
    padding: 0 0.1rem;
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
    opacity: 0.7;
  }

  .entry-list {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
</style>
