<script lang="ts">
  import { vaultApi } from '$lib/api/vault'
  import type { VaultSearchHit } from '$lib/types/vault'

  let {
    onResults,
    onClear,
  }: {
    onResults: (hits: VaultSearchHit[]) => void
    onClear: () => void
  } = $props()

  let query = $state('')
  let loading = $state(false)
  let timer: ReturnType<typeof setTimeout> | null = null

  async function runSearch() {
    if (query.trim().length === 0) {
      onClear()
      return
    }
    loading = true
    try {
      const hits = await vaultApi.search(query.trim(), 20)
      onResults(hits)
    } finally {
      loading = false
    }
  }

  function onInput() {
    if (timer) clearTimeout(timer)
    timer = setTimeout(runSearch, 300)
  }
</script>

<div class="search-box">
  <svg class="search-icon" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
    <circle cx="11" cy="11" r="7" />
    <line x1="21" y1="21" x2="16.65" y2="16.65" />
  </svg>
  <input
    class="search-input"
    type="search"
    placeholder="搜索标题 / 用户名 / 标签"
    bind:value={query}
    oninput={onInput}
  />
  {#if loading}<span class="search-status">...</span>{/if}
</div>

<style>
  .search-box {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.25rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 0.3rem);
    padding: 0 0.35rem;
    height: 1.75rem;
    flex-shrink: 0;
  }

  .search-icon {
    flex-shrink: 0;
    opacity: 0.35;
    color: var(--text-primary);
  }

  .search-input {
    flex: 1;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: var(--font-sm, 0.65rem);
    font-family: inherit;
    outline: none;
    padding: 0;
    min-width: 0;
  }

  .search-input::placeholder {
    color: var(--text-faint);
  }

  .search-input::-webkit-search-cancel-button {
    -webkit-appearance: none;
  }

  .search-status {
    font-size: var(--font-sm, 0.6rem);
    color: var(--text-faint);
    flex-shrink: 0;
  }
</style>
