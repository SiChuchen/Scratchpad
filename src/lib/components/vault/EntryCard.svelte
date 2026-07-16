<script lang="ts">
  import TagEditor from './TagEditor.svelte'
  import { vaultApi } from '$lib/api/vault'
  import type { VaultEntryDetail } from '$lib/types/vault'

  let { entryId }: { entryId: string } = $props()

  let detail = $state<VaultEntryDetail | null>(null)
  let showSecret = $state<Record<string, boolean>>({})

  async function load() {
    detail = await vaultApi.getEntry(entryId)
  }

  async function copy(text: string) {
    await navigator.clipboard.writeText(text).catch(() => {})
  }

  async function remove() {
    if (!confirm('确认删除？')) return
    await vaultApi.deleteEntry(entryId)
    window.dispatchEvent(new CustomEvent('vault-entry-deleted', { detail: entryId }))
  }

  $effect(() => {
    void entryId
    load()
  })
</script>

{#if detail}
  <div class="entry-card">
    <div class="entry-header">
      <span class="title">{detail.entry.title}</span>
      <span class="kind-badge">{detail.entry.kind}</span>
      <div class="spacer"></div>
      <button class="icon-btn danger" onclick={remove} title="删除" aria-label="删除">
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="3 6 5 6 21 6"></polyline>
          <path d="M19 6l-2 14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2L5 6"></path>
        </svg>
      </button>
    </div>

    {#if detail.fields.length > 0}
      <div class="fields">
        {#each detail.fields as f (f.key)}
          <div class="field">
            <span class="field-key">{f.key}</span>
            <code class="field-value">
              {#if f.isSensitive && !showSecret[f.key]}••••••••{:else}{f.value}{/if}
            </code>
            <div class="field-actions">
              {#if f.isSensitive}
                <button class="mini-btn" onclick={() => showSecret[f.key] = !showSecret[f.key]} title={showSecret[f.key] ? '隐藏' : '显示'}>
                  {showSecret[f.key] ? '隐藏' : '显示'}
                </button>
              {/if}
              <button class="mini-btn" onclick={() => copy(f.value)} title="拷贝">拷贝</button>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    {#if detail.entry.notes}
      <div class="notes">{detail.entry.notes}</div>
    {/if}

    <TagEditor entryId={entryId} tags={detail.tags} />
  </div>
{/if}

<style>
  .entry-card {
    background: var(--surface-1);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-lg, 0.5rem);
    padding: 0.5rem 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .entry-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .title {
    font-size: var(--font-sm, 0.75rem);
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .kind-badge {
    font-size: 0.6rem;
    color: var(--text-muted);
    padding: 0.1rem 0.4rem;
    background: var(--surface-2);
    border-radius: var(--radius-md, 0.25rem);
    flex-shrink: 0;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .spacer {
    flex: 1;
  }

  .icon-btn {
    background: none;
    border: 1px solid transparent;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0.2rem;
    border-radius: var(--radius-md, 0.25rem);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
  }

  .icon-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
  }

  .icon-btn.danger:hover {
    color: #ff6b6b;
    background: color-mix(in srgb, #ff6b6b 15%, transparent);
  }

  .fields {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .field {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-height: 1.4rem;
  }

  .field-key {
    width: 5rem;
    flex-shrink: 0;
    color: var(--text-muted);
    font-size: var(--font-sm, 0.65rem);
  }

  .field-value {
    flex: 1;
    color: var(--text-primary);
    font-size: var(--font-sm, 0.7rem);
    font-family: var(--font-family-mono, "Cascadia Code", "Consolas", monospace);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .field-actions {
    display: flex;
    gap: 0.2rem;
    flex-shrink: 0;
  }

  .mini-btn {
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    color: var(--text-muted);
    font-size: 0.6rem;
    padding: 0.15rem 0.4rem;
    border-radius: var(--radius-md, 0.25rem);
    cursor: pointer;
    font-family: inherit;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
  }

  .mini-btn:hover {
    color: var(--color-primary);
    border-color: color-mix(in srgb, var(--color-primary) 30%, transparent);
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
  }

  .notes {
    font-size: var(--font-sm, 0.65rem);
    color: var(--text-muted);
    white-space: pre-wrap;
    line-height: 1.45;
    padding-top: 0.1rem;
    border-top: 1px solid var(--border-subtle);
  }
</style>
