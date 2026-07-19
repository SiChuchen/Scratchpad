<script lang="ts">
  import { messages } from '$lib/i18n'
  import type { ContentDetail } from '$lib/types/content'

  interface Props {
    detail: ContentDetail
    resetToken: string | number
    onCopyText: (text:string,sensitive:boolean)=>Promise<void>
    onCopyFile: (path:string,kind:'image'|'file')=>Promise<void>
    onOpen: (target:string)=>Promise<void>
    onManage: (id:string)=>Promise<void>
    onNotify: (message:string,kind?:'success'|'error')=>void
  }
  let {detail,resetToken,onCopyText,onCopyFile,onOpen,onManage,onNotify}:Props=$props()
  let revealed=$state<number[]>([])
  $effect(()=>{ resetToken; detail.summary.id; revealed=[] })
  const fields=$derived('fields' in detail?[...detail.fields].sort((a,b)=>a.sortOrder-b.sortOrder):[])
  const primarySensitive=$derived(detail.kind==='credential'?(fields.find(f=>f.isSensitive)??fields[0]??null):null)

  async function run(operation:()=>Promise<void>,success=messages.workspace.copied){try{await operation();onNotify(success,'success')}catch{onNotify(messages.workspace.notices.copyFailed,'error')}}
  function primary(){
    if(detail.kind==='text')return run(()=>onCopyText(detail.body,false))
    if(detail.kind==='note')return run(()=>onCopyText(detail.body,false))
    if(detail.kind==='image')return run(()=>onCopyFile(detail.assetPath,'image'))
    if(detail.kind==='file')return run(()=>onCopyFile(detail.assetPath,'file'))
    if(detail.kind==='bookmark')return run(()=>onOpen(detail.url),messages.workspace.openLink)
    if(primarySensitive)return run(()=>onCopyText(primarySensitive.value,primarySensitive.isSensitive))
  }
  const primaryLabel=$derived(detail.kind==='text'?messages.quickAccess.copyText:detail.kind==='note'?messages.quickAccess.copyNote:detail.kind==='image'?messages.quickAccess.copyImage:detail.kind==='file'?messages.quickAccess.copyFile:detail.kind==='bookmark'?messages.workspace.openLink:messages.quickAccess.copyPassword)
</script>

<article class="quick-detail">
  <header>
    <div><span class="retention">{detail.summary.retention==='saved'?messages.workspace.savedLabel:messages.workspace.temporary}</span><h2>{detail.summary.title}</h2></div>
    <button type="button" class="primary-action" onclick={primary} disabled={(detail.kind==='image'||detail.kind==='file')&&!detail.available}>{primaryLabel}</button>
  </header>

  {#if detail.kind==='text'}<pre class="body">{detail.body}</pre>
  {:else if detail.kind==='image'||detail.kind==='file'}<div class="file-info"><strong>{detail.fileName}</strong>{#if !detail.available}<span>{detail.kind==='image'?messages.workspace.unavailableImage:messages.workspace.unavailableFile}</span>{/if}</div>
  {:else}
    <section aria-label={messages.workspace.usefulInformation}>
      {#if detail.kind==='bookmark'}<div class="field-row" data-field-row><span>URL</span><strong>{detail.url}</strong><button type="button" data-copy-action class="quick-copy-target" onclick={()=>run(()=>onCopyText(detail.url,false))}>{messages.workspace.copy}</button></div>{/if}
      {#each fields as field,index (field.key+index)}
        <div class="field-row" data-field-row>
          <span>{field.key}</span><strong>{field.isSensitive&&!revealed.includes(index)?'••••••••':field.value}</strong>
          {#if field.isSensitive}<button type="button" class="reveal" onclick={()=>revealed=revealed.includes(index)?revealed.filter(x=>x!==index):[...revealed,index]}>{revealed.includes(index)?messages.workspace.hide:messages.workspace.reveal}</button>{/if}
          <button type="button" data-copy-action class="quick-copy-target" aria-label={`${messages.workspace.copy} ${field.key}`} onclick={()=>run(()=>onCopyText(field.value,field.isSensitive))}>{messages.workspace.copy}</button>
        </div>
      {/each}
    </section>
    {#if detail.kind==='note'}<section class="support"><h3>{messages.workspace.notes}</h3><p>{detail.body}</p><button type="button" data-copy-action class="quick-copy-target" onclick={()=>run(()=>onCopyText(detail.body,false))}>{messages.workspace.copy}</button></section>{/if}
    {#if 'notes' in detail&&detail.notes}<section class="support"><h3>{messages.workspace.notes}</h3><p>{detail.notes}</p><button type="button" data-copy-action class="quick-copy-target" onclick={()=>run(()=>onCopyText(detail.notes!,false))}>{messages.workspace.copy}</button></section>{/if}
    {#if detail.tags.length}<section class="support"><h3>{messages.workspace.tags}</h3><p>{detail.tags.map(t=>t.tag).join(' · ')}</p></section>{/if}
  {/if}
  <footer><button type="button" class="manage" onclick={()=>run(()=>onManage(detail.summary.id),messages.quickAccess.openedInMain)}>{messages.quickAccess.manageInMain}</button></footer>
</article>

<style>
  .quick-detail{display:flex;flex-direction:column;gap:.75rem;min-width:0;padding:.7rem;color:var(--text-primary)}header{display:flex;align-items:center;justify-content:space-between;gap:.65rem}h2{margin:.15rem 0 0;font-size:max(var(--font-lg,1rem),1rem);overflow-wrap:anywhere}.retention{font-size:var(--font-xs,.65rem);color:var(--color-primary)}button{min-height:2.4rem;padding:.4rem .7rem;border:1px solid var(--border-default);border-radius:var(--radius-md);background:var(--surface-2);color:var(--text-primary);font:inherit;cursor:pointer}.primary-action{min-width:6.5rem;background:var(--color-primary);color:var(--color-on-primary,#fff);border-color:var(--color-primary);font-weight:700}.body,.file-info,.support p{white-space:pre-wrap;overflow-wrap:anywhere;font:inherit;font-size:max(var(--font-md,.88rem),.88rem);line-height:1.55}.field-row{display:grid;grid-template-columns:minmax(4rem,.45fr) minmax(0,1fr) auto auto;align-items:center;gap:.4rem;min-height:3rem;border-bottom:1px solid var(--border-subtle);font-size:max(var(--font-md,.88rem),.88rem)}.field-row>span{color:var(--text-muted)}.field-row strong{overflow-wrap:anywhere}.quick-copy-target{width:auto;min-width:3.3rem;min-height:2.4rem;margin-inline-start:auto;font-weight:650}.reveal{min-width:2.4rem;padding:.35rem}.support{position:relative;padding-top:.2rem}.support h3{margin:.2rem 0;color:var(--text-muted);font-size:var(--font-sm,.75rem)}.support .quick-copy-target{position:absolute;right:0;top:0}footer{display:flex;justify-content:flex-end;padding-top:.2rem}.manage{background:transparent}@media(max-width:520px){.field-row{grid-template-columns:minmax(3.5rem,.4fr) minmax(0,1fr) auto}.field-row .reveal{grid-column:2}.field-row .quick-copy-target{grid-column:3;grid-row:1 / span 2}}
</style>
