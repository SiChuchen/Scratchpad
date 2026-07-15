<script lang="ts">
  import { vaultApi } from '$lib/api/vault'

  let { entryId, tags }: { entryId: string; tags: string[] } = $props()

  let editing = $state(false)
  // draft 仅在进入编辑模式时从 tags 重新填充（见 onclick），初始值用占位符避免 linter 警告
  let draft = $state('')

  async function save() {
    const list = draft.split(',').map(s => s.trim()).filter(Boolean)
    await vaultApi.updateTags(entryId, list)
    editing = false
    window.dispatchEvent(new CustomEvent('vault-tags-changed', { detail: entryId }))
  }

  async function retag() {
    await vaultApi.retag(entryId)
  }
</script>

<div class="tag-editor">
  {#if editing}
    <input bind:value={draft} placeholder="逗号分隔" />
    <button onclick={save}>保存</button>
    <button onclick={() => editing = false}>取消</button>
  {:else}
    {#each tags as t}
      <span class="tag">{t}</span>
    {/each}
    <button class="tag-action" onclick={() => { draft = tags.join(', '); editing = true }}>编辑</button>
    <button class="tag-action" onclick={retag}>重新打标</button>
  {/if}
</div>

<style>
  .tag-editor { display: flex; gap: 4px; flex-wrap: wrap; align-items: center; }
  .tag { background: rgba(0,0,0,0.08); padding: 2px 8px; border-radius: 10px; font-size: 0.8em; }
  .tag-action { font-size: 0.75em; opacity: 0.6; cursor: pointer; background: none; border: none; }
  .tag-action:hover { opacity: 1; }
</style>
