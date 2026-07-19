<script lang="ts">
  import { onDestroy, onMount } from 'svelte'
  import { contentApi } from '$lib/api/content'
  import { dockApi } from '$lib/api/dock'
  import { vaultApi } from '$lib/api/vault'
  import QuickContentDetail from '$lib/components/content/QuickContentDetail.svelte'
  import { messages } from '$lib/i18n'
  import {
    UnifiedSearchController,
    type ContentSearchState,
  } from '$lib/state/content-search'
  import type { ContentDetail } from '$lib/types/content'
  import SearchResultList from './SearchResultList.svelte'

  interface Props {
    notify: (
      text: string,
      kind?: 'success' | 'error',
      undo?: () => void,
      actionLabel?: string,
    ) => void
    resetToken?: number | string
    refreshToken?: number | string
    autoHybridSearch?: boolean
  }

  let {
    notify,
    resetToken = 0,
    refreshToken = 0,
    autoHybridSearch = false,
  }: Props = $props()

  let query = $state('')
  let hits = $state<ContentSearchState['hits']>([])
  let selectedId = $state<string | null>(null)
  let understoodTerms = $state<string[]>([])
  let phase = $state<ContentSearchState['phase']>('idle')
  let errorMessage = $state<string | null>(null)
  let selectedDetail = $state<ContentDetail | null>(null)
  let detailLoading = $state(false)
  let detailError = $state<string | null>(null)
  let detailRevision = 0
  let controller: UnifiedSearchController | null = null

  let localResetToken = $state(0)
  let previousReset: number | string | undefined
  $effect(() => {
    const next = resetToken
    if (previousReset !== undefined && next !== previousReset) {
      localResetToken += 1
    }
    previousReset = next
  })
  const detailReset = $derived(`${localResetToken}:${selectedId ?? ''}`)

  let previousRefresh: number | string | undefined
  $effect(() => {
    const next = refreshToken
    if (previousRefresh !== undefined && next !== previousRefresh) {
      if (query.trim()) void controller?.search(query)
    }
    previousRefresh = next
  })

  $effect(() => {
    controller?.setPlannerEnabled(autoHybridSearch)
  })

  onMount(() => {
    controller = new UnifiedSearchController(contentApi, onSearchState, {
      debounceMs: 300,
      aiDelayMs: 700,
      usePlanner: autoHybridSearch,
      limit: 50,
    })
  })

  onDestroy(() => {
    controller?.dispose()
    controller = null
  })

  function onSearchState(state: ContentSearchState): void {
    hits = state.hits
    selectedId = state.selectedId
    understoodTerms = state.understoodTerms
    phase = state.phase
    errorMessage = state.error
  }

  function onQueryInput(event: Event): void {
    query = (event.currentTarget as HTMLInputElement).value
    void controller?.search(query)
  }

  $effect(() => {
    const id = selectedId
    const revision = ++detailRevision
    if (!id) {
      selectedDetail = null
      detailLoading = false
      detailError = null
      return
    }
    detailLoading = true
    detailError = null
    void contentApi.detail(id)
      .then((detail) => {
        if (revision === detailRevision) selectedDetail = detail
      })
      .catch((error: unknown) => {
        if (revision !== detailRevision) return
        selectedDetail = null
        detailError = error instanceof Error ? error.message : String(error)
      })
      .finally(() => {
        if (revision === detailRevision) detailLoading = false
      })
  })

  function onKeydown(event: KeyboardEvent): void {
    if (!hits.length || (event.key !== 'ArrowDown' && event.key !== 'ArrowUp')) return
    event.preventDefault()
    const current = hits.findIndex((hit) => hit.summary.id === selectedId)
    const next = event.key === 'ArrowDown'
      ? (current < 0 ? 0 : Math.min(current + 1, hits.length - 1))
      : (current <= 0 ? 0 : current - 1)
    onSelect(hits[next]!.summary.id)
  }

  function onSelect(id: string): void {
    selectedId = id
    controller?.select(id)
  }

  async function copyText(text: string, sensitive: boolean): Promise<void> {
    await vaultApi.copyText(text, sensitive)
  }

  async function copyFile(path: string, kind: 'image' | 'file'): Promise<void> {
    if (kind === 'image') await dockApi.copyImage(path)
    else await dockApi.copyFile(path)
  }

  async function openUrl(target: string): Promise<void> {
    const url = new URL(target)
    if (url.protocol !== 'http:' && url.protocol !== 'https:') throw new Error('unsupported URL')
    window.open(url.href, '_blank', 'noopener,noreferrer')
  }

  async function manageInMain(_id: string): Promise<void> {
    notify(messages.quickAccess.manageInMain)
  }

  const statusText = $derived.by(() => {
    if (phase === 'planning') return messages.quickAccess.aiEnhancing
    if (phase === 'expanded' && understoodTerms.length) {
      return messages.library.aiUnderstanding.replace('{terms}', understoodTerms.join('、'))
    }
    if (phase === 'error') return errorMessage ?? messages.toast.operationFailed
    return ''
  })
</script>

<section class="mode mode-search">
  <header class="mode-header">
    <h2>{messages.quickAccess.search}</h2>
    <span class="hint">Ctrl+Tab → {messages.quickAccess.record}</span>
  </header>

  <input
    class="search-input"
    type="search"
    placeholder={messages.quickAccess.searchPlaceholder}
    value={query}
    oninput={onQueryInput}
    onkeydown={onKeydown}
    aria-label={messages.quickAccess.search}
  />

  {#if statusText}
    <div class="status" class:error={phase === 'error'} aria-live="polite">{statusText}</div>
  {/if}

  <div class="dual-pane">
    <div class="left-pane">
      {#if hits.length}
        <SearchResultList {hits} {selectedId} onSelect={onSelect} />
      {:else}
        <div class="empty-list"><p class="muted">{query ? messages.quickAccess.noResults : messages.quickAccess.searchPlaceholder}</p></div>
      {/if}
    </div>

    <div class="right-pane">
      {#if detailLoading}
        <div class="detail-state" aria-live="polite">{messages.settings.checking}</div>
      {:else if detailError}
        <div class="detail-state error" aria-live="polite">{messages.toast.loadFailed}: {detailError}</div>
      {:else if selectedDetail}
        <QuickContentDetail
          detail={selectedDetail}
          resetToken={detailReset}
          onCopyText={copyText}
          onCopyFile={copyFile}
          onOpen={openUrl}
          onManage={manageInMain}
          onNotify={notify}
        />
      {:else}
        <div class="detail-state">{messages.quickAccess.noSelection}</div>
      {/if}
    </div>
  </div>
</section>

<style>
  .mode-search{flex:1;display:flex;flex-direction:column;gap:.5rem;padding:.75rem;min-height:0}.mode-header{display:flex;align-items:center;justify-content:space-between}.mode-header h2{margin:0;font-size:var(--font-md,15px);font-weight:600;color:var(--text-primary)}.hint{font-size:var(--font-xs,11px);color:var(--text-muted)}.search-input{width:100%;border:1px solid var(--border-default);border-radius:var(--radius-md,6px);padding:.55rem .7rem;background:var(--surface-1);color:var(--text-primary);font:inherit;font-size:var(--font-md,15px);outline:none}.search-input:focus{border-color:var(--color-primary)}.status{font-size:var(--font-xs,11px);color:var(--color-primary);padding:.25rem .4rem;border-radius:var(--radius-md,6px);background:color-mix(in srgb,var(--color-primary) 8%,transparent)}.status.error,.detail-state.error{color:var(--color-danger)}.dual-pane{flex:1;display:grid;grid-template-columns:minmax(0,.92fr) minmax(0,1.08fr);gap:.5rem;min-height:0}.left-pane,.right-pane{display:flex;flex-direction:column;min-height:0;border:1px solid var(--border-default);border-radius:var(--radius-md,6px);background:var(--surface-1);padding:.35rem}.left-pane{overflow:hidden}.right-pane{overflow:auto}.empty-list{flex:1;display:flex;align-items:center;justify-content:center}.muted,.detail-state{color:var(--text-muted);font-size:var(--font-sm,13px);margin:0;padding:.5rem}@media(max-width:540px){.dual-pane{grid-template-columns:minmax(0,1fr) minmax(0,1.15fr)}}
</style>
