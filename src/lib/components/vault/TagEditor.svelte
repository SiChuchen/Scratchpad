<script lang="ts">
  import { vaultApi } from '$lib/api/vault'

  let { entryId, tags }: { entryId: string; tags: string[] } = $props()

  let editing = $state(false)
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
    <input class="tag-input" bind:value={draft} placeholder="逗号分隔" />
    <button class="tag-action" onclick={save}>保存</button>
    <button class="tag-action" onclick={() => editing = false}>取消</button>
  {:else}
    {#each tags as t}
      <span class="tag">{t}</span>
    {/each}
    <button class="tag-action" onclick={() => { draft = tags.join(', '); editing = true }}>编辑标签</button>
    <button class="tag-action" onclick={retag} title="让 LLM 重新生成标签">重新打标</button>
  {/if}
</div>

<style>
  .tag-editor {
    display: flex;
    gap: 0.25rem;
    flex-wrap: wrap;
    align-items: center;
    padding-top: 0.2rem;
    border-top: 1px solid var(--border-subtle);
  }

  .tag {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    color: var(--color-primary);
    padding: 0.1rem 0.45rem;
    border-radius: 0.6rem;
    font-size: 0.6rem;
    font-weight: 500;
  }

  .tag-input {
    background: var(--surface-2);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius-md, 0.25rem);
    color: var(--text-primary);
    font-size: 0.65rem;
    font-family: inherit;
    padding: 0.15rem 0.4rem;
    outline: none;
    flex: 1;
    min-width: 8rem;
  }

  .tag-action {
    background: none;
    border: 1px solid transparent;
    color: var(--text-faint);
    font-size: 0.6rem;
    cursor: pointer;
    padding: 0.15rem 0.35rem;
    border-radius: var(--radius-md, 0.25rem);
    font-family: inherit;
    transition: color 0.12s, background 0.12s;
  }

  .tag-action:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
  }
</style>
