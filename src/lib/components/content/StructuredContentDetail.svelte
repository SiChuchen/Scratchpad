<script lang="ts">
  import { contentApi } from '$lib/api/content'; import { vaultApi } from '$lib/api/vault'; import type { ContentDetail } from '$lib/types/content'
  type Structured=Extract<ContentDetail,{kind:'credential'|'bookmark'|'note'}>
  interface Props{detail:Structured;resetToken:string|number;onClose:()=>void;onChanged:(id:string)=>Promise<void>;onNotify:(m:string,k?:'success'|'error')=>void;onDelete?:()=>void;onToggleSaved?:()=>void}
  let{detail,resetToken,onClose,onChanged,onNotify,onDelete,onToggleSaved}:Props=$props();let revealed=$state<string[]>([]);let editing=$state(false);let title=$state('');let notes=$state('');let body=$state('');let loadedId=$state('')
  $effect(()=>{resetToken;revealed=[]})
  $effect(()=>{if(loadedId!==detail.summary.id){loadedId=detail.summary.id;title=detail.summary.title;notes='notes'in detail?(detail.notes??''):'';body=detail.kind==='note'?detail.body:'';editing=false}})
  function value(field:{key:string;value:string;isSensitive:boolean}){return field.isSensitive&&!revealed.includes(field.key)?'••••••••':field.value}
  async function copy(text:string,sensitive=false){try{await vaultApi.copyText(text,sensitive);onNotify('已复制')}catch(e){onNotify(String(e),'error')}}
  async function save(){try{const fields='fields'in detail?detail.fields.map(f=>({key:f.key,value:f.value,isSensitive:f.isSensitive})):[];await contentApi.updateStructured(detail.summary.id,{kind:detail.kind,title,fields,notes:detail.kind==='note'?body:(notes||null),manualTags:detail.tags.filter(t=>t.source==='manual').map(t=>t.tag)});editing=false;await onChanged(detail.summary.id);onNotify('已更新')}catch(e){onNotify(String(e),'error')}}
</script>
<header><button type="button" onclick={onClose}>返回</button><strong>{detail.summary.title}</strong></header>
<main>
  {#if editing}<label>标题<input bind:value={title}/></label>{#if detail.kind==='note'}<label>正文<textarea bind:value={body}></textarea></label>{:else}<label>备注<textarea bind:value={notes}></textarea></label>{/if}<div class="actions"><button type="button" onclick={save}>保存</button><button type="button" onclick={()=>editing=false}>取消</button></div>
  {:else}
    <section aria-label="可直接使用的信息"><h3>可直接使用的信息</h3>
      {#if detail.kind==='bookmark'}<div class="field" data-field-row><span>链接</span><a href={detail.url}>{detail.url}</a><button data-copy-action type="button" onclick={()=>copy(detail.url)}>复制</button></div>{/if}
      {#if detail.kind==='note'}<div class="note-body">{detail.body}</div>{/if}
      {#if 'fields' in detail}{#each [...detail.fields].sort((a,b)=>a.sortOrder-b.sortOrder) as field}<div class="field" data-field-row><span>{field.key}</span><strong>{value(field)}</strong>{#if field.isSensitive}<button type="button" onclick={()=>revealed=revealed.includes(field.key)?revealed.filter(k=>k!==field.key):[...revealed,field.key]}>{revealed.includes(field.key)?'隐藏':'显示'}</button>{/if}<button data-copy-action class="copy" type="button" onclick={()=>copy(field.value,field.isSensitive)}>复制</button></div>{/each}{/if}
    </section>
    {#if 'notes'in detail&&detail.notes}<section><h3>备注</h3><p>{detail.notes}</p></section>{/if}
    {#if detail.tags.length}<section><h3>标签</h3><p>{detail.tags.map(t=>t.tag).join(' · ')}</p></section>{/if}
    <div class="actions"><button type="button" onclick={()=>editing=true}>{detail.kind==='note'?'编辑备注':'编辑'}</button>{#if detail.kind==='bookmark'}<button type="button" onclick={()=>window.open(detail.url,'_blank')}>打开链接</button>{/if}{#if onToggleSaved}<button type="button" onclick={onToggleSaved}>{detail.summary.retention==='saved'?'取消收藏':'收藏'}</button>{/if}{#if onDelete}<button type="button" onclick={onDelete}>删除</button>{/if}</div>
  {/if}
</main>
<style>header{position:sticky;top:0;display:flex;align-items:center;gap:.5rem;padding:.6rem;border-bottom:1px solid var(--border-subtle);background:var(--surface-0);z-index:1}main{display:flex;flex-direction:column;gap:.8rem;padding:.8rem}h3{margin:.15rem 0 .4rem;color:var(--color-primary);font-size:var(--font-sm,.72rem)}.field{display:grid;grid-template-columns:minmax(4rem,.5fr) minmax(0,1fr) auto auto;align-items:center;gap:.4rem;padding:.38rem 0;border-bottom:1px solid var(--border-subtle);font-size:max(var(--font-md,.85rem),.85rem);line-height:1.5}.field>span{color:var(--text-muted)}.field strong,.field a{overflow-wrap:anywhere}.field .copy{margin-inline-start:auto}.note-body,p{white-space:pre-wrap;overflow-wrap:anywhere;font-size:max(var(--font-md,.85rem),.85rem);line-height:1.5}.actions{display:flex;flex-wrap:wrap;justify-content:flex-end;gap:.35rem}button,input,textarea{min-height:2.25rem;padding:.35rem .6rem;border:1px solid var(--border-default);border-radius:var(--radius-md);background:var(--surface-1);color:var(--text-primary);font:inherit}label{display:flex;flex-direction:column;gap:.25rem}input,textarea{box-sizing:border-box;width:100%}textarea{min-height:7rem}@media(max-width:360px){.field{grid-template-columns:minmax(3rem,.4fr) minmax(0,1fr) auto}.field button:not(.copy){grid-column:2}.field .copy{grid-column:3;grid-row:1 / span 2}}</style>
