<script lang="ts">
  import ContentSummaryCard from './ContentSummaryCard.svelte'
  import type { ContentSummary } from '$lib/types/content'
  interface Props { items:ContentSummary[]; selectedId:string|null; reorderable:boolean; busyIds?:string[]; onSelect:(id:string)=>void; onReorder:(ids:string[])=>Promise<void>; onToggleSaved:(item:ContentSummary)=>void; onCopy:(item:ContentSummary)=>void; onDelete:(item:ContentSummary)=>void }
  let { items, selectedId, reorderable, busyIds = [], onSelect, onReorder, onToggleSaved, onCopy, onDelete }: Props = $props()
  let dragging = $state<string|null>(null)
  function keydown(e:KeyboardEvent) { const i=items.findIndex(x=>x.id===selectedId); if(e.key==='ArrowDown'){e.preventDefault();onSelect(items[Math.min(items.length-1,i+1)]?.id)} else if(e.key==='ArrowUp'){e.preventDefault();onSelect(items[Math.max(0,i<0?0:i-1)]?.id)} else if(e.key==='Enter'&&selectedId){e.preventDefault();onSelect(selectedId)} }
  async function drop(id:string) { if(!dragging||dragging===id)return; const ids=items.map(x=>x.id); const from=ids.indexOf(dragging),to=ids.indexOf(id); ids.splice(to,0,ids.splice(from,1)[0]); dragging=null; await onReorder(ids) }
</script>
<div class="list" role="listbox" tabindex="0" onkeydown={keydown}>
  {#each items as item (item.id)}
    <div draggable={reorderable && item.capabilities.reorder} ondragstart={() => dragging=item.id} ondragover={(e)=>{if(reorderable)e.preventDefault()}} ondrop={() => drop(item.id)}>
      <ContentSummaryCard {item} selected={selectedId===item.id} busy={busyIds.includes(item.id)} draggable={reorderable} {onSelect} {onToggleSaved} {onCopy} {onDelete}/>
    </div>
  {/each}
</div>
<style>.list{display:flex;flex-direction:column;gap:.38rem;outline:none}.list:focus-visible{outline:2px solid var(--color-primary);outline-offset:2px}</style>
