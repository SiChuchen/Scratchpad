<script lang="ts">
  import ContentKindIcon from '$lib/components/content/ContentKindIcon.svelte'
  import { messages } from '$lib/i18n'
  import type { ContentSearchHit } from '$lib/types/content'
  interface Props { hits:ContentSearchHit[]; selectedId:string|null; onSelect:(id:string)=>void }
  let {hits,selectedId,onSelect}:Props=$props(); let listEl:HTMLDivElement|null=$state(null)
  $effect(()=>{selectedId;hits;if(!listEl)return;const selected=listEl.querySelector<HTMLElement>('[aria-selected="true"]');selected?.scrollIntoView?.({block:'nearest'})})
</script>
<div class="search-result-list" bind:this={listEl} role="listbox" aria-label={messages.quickAccess.search}>
  {#each hits as hit (hit.summary.id)}
    <button type="button" class="result-row" class:selected={hit.summary.id===selectedId} role="option" aria-selected={hit.summary.id===selectedId} onclick={()=>onSelect(hit.summary.id)}>
      <ContentKindIcon kind={hit.summary.kind}/><span class="content"><strong>{hit.summary.title}</strong>{#if hit.summary.preview}<span>{hit.summary.preview}</span>{/if}</span>
      <span class="retention">{hit.summary.retention==='saved'?messages.workspace.savedLabel:messages.workspace.temporary}</span>
    </button>
  {/each}
</div>
<style>
  .search-result-list{flex:1;min-height:0;display:flex;flex-direction:column;gap:.2rem;overflow-y:auto}.result-row{width:100%;display:grid;grid-template-columns:auto minmax(0,1fr) auto;align-items:center;gap:.45rem;padding:.48rem;border:1px solid transparent;border-radius:var(--radius-md);background:transparent;color:var(--text-primary);font:inherit;text-align:start;cursor:pointer}.result-row:hover{background:var(--surface-2)}.result-row.selected{border-color:var(--color-primary);background:color-mix(in srgb,var(--color-primary) 12%,transparent)}.content{min-width:0;display:flex;flex-direction:column;gap:.12rem}.content strong,.content span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.content strong{font-size:max(var(--font-sm,.78rem),.78rem)}.content span,.retention{color:var(--text-muted);font-size:var(--font-xs,.64rem)}.retention{white-space:nowrap}
</style>
