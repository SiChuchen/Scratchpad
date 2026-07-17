<script lang="ts">
  // src/lib/components/vault/LibrarySearchInput.svelte
  //
  // Library 搜索输入框 + 混合检索状态展示。
  //
  // 行为：
  //   * 用户输入时防抖 300ms（避免每个 keystroke 都触发 searchLocal）；
  //   * 非 query 状态调 onClear 让父组件回到非搜索列表态；
  //   * Ctrl+Enter 触发即时 planSearch（跳过 700ms 防抖）；
  //   * aria-live="polite" 区域显示 AI 理解 / 本地降级 / 错误状态；
  //   * 该状态只在用户开始搜索后显示（不抢占初始 focus）。
  //
  // 父组件（VaultView）负责把 HybridSearchController 创建好并传入；本组件
  // 只负责驱动 controller.search() + 把 controller 发布的 state 翻译成
  // 用户可见字符串。

  import { onMount } from 'svelte'
  import { messages } from '$lib/i18n'
  import type { HybridSearchController, HybridSearchState } from '$lib/state/vault-search'

  interface Props {
    controller: HybridSearchController
    /** 是否在非空 query 时启用 AI plan 扩展（autoHybridSearch）。 */
    autoHybridSearch: boolean
    /**
     * 父组件从 controller.onState 收到的最新 state（每帧通过 prop 注入）。
     * null 表示尚未发起任何搜索。
     */
    searchState?: HybridSearchState | null
    /** 告知父组件当前是否处于"已开始搜索"态。 */
    onStartedChange?: (started: boolean) => void
    /** 当前 query 文本变化（用于父组件保持 filter 与 query 互不干扰）。 */
    onQueryChange?: (query: string) => void
  }

  let {
    controller,
    autoHybridSearch,
    searchState = null,
    onStartedChange,
    onQueryChange,
  }: Props = $props()

  let query = $state('')
  let inputTimer: ReturnType<typeof setTimeout> | null = null

  function notifyStarted(started: boolean) {
    onStartedChange?.(started)
  }

  function notifyQuery(q: string) {
    onQueryChange?.(q)
  }

  function commitSearch() {
    const trimmed = query.trim()
    if (trimmed.length === 0) {
      notifyStarted(false)
      notifyQuery('')
      return
    }
    notifyStarted(true)
    notifyQuery(trimmed)
    // I3: 当 autoHybridSearch=false 时只走本地搜索，跳过 planSearch。
    // 这避免了一次必然失败的 plan 请求与 700ms 延迟。
    if (autoHybridSearch) {
      void controller.search(trimmed)
    } else {
      void controller.searchLocalOnly(trimmed)
    }
  }

  function onInput() {
    if (inputTimer) clearTimeout(inputTimer)
    inputTimer = setTimeout(() => {
      inputTimer = null
      commitSearch()
    }, 300)
  }

  function onKeydown(e: KeyboardEvent) {
    // Ctrl+Enter 立即触发 planSearch（跳过 700ms 延迟）。
    // 当 autoHybridSearch=false 时降级为本地即时搜索。
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault()
      if (inputTimer) {
        clearTimeout(inputTimer)
        inputTimer = null
      }
      const trimmed = query.trim()
      if (trimmed.length === 0) return
      notifyStarted(true)
      notifyQuery(trimmed)
      if (autoHybridSearch) {
        void controller.search(trimmed)
      } else {
        void controller.searchLocalOnly(trimmed)
      }
    }
  }

  function clear() {
    query = ''
    if (inputTimer) {
      clearTimeout(inputTimer)
      inputTimer = null
    }
    notifyStarted(false)
    notifyQuery('')
  }

  // I6: 卸载时清理未触发的 inputTimer，避免组件销毁后回调访问已释放状态。
  onMount(() => {
    return () => {
      if (inputTimer) {
        clearTimeout(inputTimer)
        inputTimer = null
      }
    }
  })

  // 父组件需要把 controller 发布的最新 state 同步过来；通过 prop 注入
  // 即可（Svelte 5 中 reactive prop 会自动重算 derived）。
  const statusText = $derived(computeStatusText(autoHybridSearch, searchState))

  const showStatus = $derived(
    autoHybridSearch &&
      searchState !== null &&
      searchState.query.trim().length > 0,
  )

  function computeStatusText(
    enabled: boolean,
    s: HybridSearchState | null,
  ): string {
    if (!enabled) return ''
    if (!s) return ''
    if (s.phase === 'expanded' && s.understoodTerms.length > 0) {
      return messages.library.aiUnderstanding.replace('{terms}', s.understoodTerms.join('、'))
    }
    if (s.phase === 'planning') {
      return messages.quickAccess.aiEnhancing
    }
    if (s.phase === 'error') {
      return messages.quickAccess.aiFallback
    }
    return messages.library.localOnly
  }
</script>

<div class="search-box">
  <svg class="search-icon" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
    <circle cx="11" cy="11" r="7" />
    <line x1="21" y1="21" x2="16.65" y2="16.65" />
  </svg>
  <input
    class="search-input"
    type="search"
    placeholder={messages.library.searchPlaceholder}
    aria-label={messages.library.title}
    bind:value={query}
    oninput={onInput}
    onkeydown={onKeydown}
  />
  {#if query}
    <button
      type="button"
      class="search-clear"
      onclick={clear}
      title={messages.settings.clear}
      aria-label={messages.settings.clear}
    >✕</button>
  {/if}
</div>

{#if showStatus}
  <div class="ai-status" aria-live="polite" role="status">{statusText}</div>
{/if}

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
    height: 1.4rem;
    min-width: 0;
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

  .search-clear {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 0.65rem;
    cursor: pointer;
    padding: 0;
    line-height: 1;
    font-family: inherit;
  }

  .search-clear:hover {
    color: var(--text-primary);
  }

  .ai-status {
    font-size: var(--font-sm, 0.6rem);
    color: var(--text-muted);
    padding: 0.1rem 0.15rem;
    line-height: 1.35;
    word-break: break-word;
  }
</style>
