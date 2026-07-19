<script lang="ts">
  import ContentKindIcon from './ContentKindIcon.svelte'
  import type { ContentSummary } from '$lib/types/content'
  interface Props { item: ContentSummary; selected: boolean; busy: boolean; draggable?: boolean; onSelect: (id:string)=>void; onToggleSaved:(item:ContentSummary)=>void; onCopy:(item:ContentSummary)=>void; onDelete:(item:ContentSummary)=>void }
  let { item, selected, busy, draggable = false, onSelect, onToggleSaved, onCopy, onDelete }: Props = $props()
  const kindLabel = $derived(({text:'文本',image:'图片',file:'文件',credential:'凭据',bookmark:'书签',note:'笔记'} as const)[item.kind])
  const copyable = $derived(item.capabilities.copyText || item.capabilities.copyImage || item.capabilities.copyFile || item.capabilities.copyPath)
  function stop(action:()=>void) { return (event: MouseEvent) => { event.stopPropagation(); action() } }
</script>

<article class="card" class:selected aria-current={selected ? 'true' : undefined}>
  <button class="select" type="button" onclick={() => onSelect(item.id)} aria-label={`打开 ${item.title}`}>
    <ContentKindIcon kind={item.kind} />
    <span class="main"><strong>{item.title || '未命名'}</strong>{#if item.preview}<span class="preview">{item.preview}</span>{/if}</span>
  </button>
  <div class="meta"><span>{kindLabel}</span><span>{item.retention === 'saved' ? '已收藏' : item.cleanupAt ? `临时保留至 ${new Date(item.cleanupAt).toLocaleDateString()}` : '临时'}</span></div>
  <div class="actions">
    {#if draggable && item.capabilities.reorder}<button type="button" class="drag" aria-label="拖动排序" disabled={busy}>⋮⋮</button>{/if}
    {#if copyable}<button type="button" onclick={stop(() => onCopy(item))} disabled={busy}>复制</button>{/if}
    {#if item.capabilities.save}<button type="button" aria-pressed="false" onclick={stop(() => onToggleSaved(item))} disabled={busy}>收藏</button>{/if}
    {#if item.capabilities.unsave}<button type="button" aria-pressed="true" onclick={stop(() => onToggleSaved(item))} disabled={busy}>取消收藏</button>{/if}
    {#if item.capabilities.delete}<button type="button" class="danger" onclick={stop(() => onDelete(item))} disabled={busy}>删除</button>{/if}
  </div>
</article>

<style>
  .card { display:grid; grid-template-columns:minmax(0,1fr) auto; gap:.25rem .5rem; padding:.55rem; border:1px solid var(--border-subtle); border-radius:var(--radius-lg,.5rem); background:var(--surface-1); cursor:pointer; }
  .card:hover,.card.selected { border-color:var(--color-primary); background:color-mix(in srgb,var(--color-primary) 7%,var(--surface-1)); }
  .select { min-width:0; display:flex; align-items:flex-start; gap:.45rem; padding:0; border:0; color:inherit; background:none; text-align:start; font:inherit; cursor:pointer; }
  .main { min-width:0; display:flex; flex-direction:column; gap:.12rem; }
  strong { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:max(var(--font-md,.82rem),.82rem); }
  .preview { display:-webkit-box; overflow:hidden; line-clamp:2; -webkit-line-clamp:2; -webkit-box-orient:vertical; color:var(--text-muted); font-size:max(var(--font-sm,.72rem),.72rem); line-height:1.4; }
  .meta { grid-column:1; display:flex; gap:.5rem; padding-inline-start:1.85rem; color:var(--text-faint); font-size:var(--font-xs,.62rem); }
  .actions { grid-column:2; grid-row:1 / span 2; display:flex; align-items:center; justify-content:flex-end; gap:.18rem; }
  .actions button { min-width:2rem; min-height:2rem; padding:.2rem .38rem; border:1px solid var(--border-subtle); border-radius:var(--radius-md,.3rem); background:var(--surface-2); color:var(--text-primary); font:inherit; font-size:var(--font-xs,.65rem); cursor:pointer; }
  .actions .danger:hover { color:var(--color-danger); }
  button:focus-visible,.card:focus-within { outline:2px solid var(--color-primary); outline-offset:2px; }
  @media (max-width:360px) { .card { grid-template-columns:minmax(0,1fr); } .actions { grid-column:1; grid-row:auto; } }
</style>
