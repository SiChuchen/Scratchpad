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

<div class="search-bar">
  <input
    type="search"
    placeholder="搜索标题/用户名/标签（FTS5 优先，未命中自动调用 LLM）"
    bind:value={query}
    oninput={onInput}
  />
  {#if loading}<span class="searching">搜索中...</span>{/if}
</div>

<style>
  .search-bar { display: flex; gap: 6px; align-items: center; }
  .search-bar input { flex: 1; padding: 6px 8px; }
  .searching { font-size: 0.8em; opacity: 0.6; }
</style>
