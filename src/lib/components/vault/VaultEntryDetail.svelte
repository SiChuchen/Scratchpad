<script lang="ts">
  // src/lib/components/vault/VaultEntryDetail.svelte
  //
  // Vault 条目只读详情视图。所有独立值（title / notes / 每个 tag / 每个
  // field）都用 CopyableValue 渲染，统一交互。
  //
  // 标签去重：按 normalizedTag 合并；当 manual + ai 同时存在时优先 manual
  // 源（显示为 "manual" badge 而非 "AI" badge）。

  import CopyableValue from './CopyableValue.svelte'
  import { messages } from '$lib/i18n'
  import type {
    VaultEntryDetail,
    VaultTag,
  } from '$lib/types/vault'

  interface Props {
    detail: VaultEntryDetail
    resetToken?: number | string
    prominent?: boolean
    onCopy: (payload: {
      label: string
      value: string
      sensitive: boolean
    }) => void | Promise<void>
    onRemoveAiTag?: (normalizedTag: string) => void | Promise<void>
  }

  let {
    detail,
    resetToken,
    prominent = false,
    onCopy,
    onRemoveAiTag,
  }: Props = $props()

  // 按 normalizedTag 去重：同一 normalized 的 manual 优先于 ai
  const dedupedTags = $derived.by(() => {
    const map = new Map<string, VaultTag>()
    for (const t of detail.tags) {
      const existing = map.get(t.normalizedTag)
      if (!existing) {
        map.set(t.normalizedTag, t)
        continue
      }
      // manual 优先
      if (existing.source === 'ai' && t.source === 'manual') {
        map.set(t.normalizedTag, t)
      }
    }
    return Array.from(map.values())
  })

  const hasNotes = $derived(
    detail.entry.notes != null && detail.entry.notes.trim().length > 0,
  )

  const sortedFields = $derived(
    [...detail.fields].sort((a, b) => a.sortOrder - b.sortOrder),
  )

  // Title / notes labels come from the shared library i18n block so they stay
  // in sync with the editor / capture preview.
  const titleLabel = $derived(messages.library.titleLabel)
  const notesLabel = $derived(messages.library.notesLabel)
</script>

<div class="detail" class:prominent>
  {#if prominent}
    <CopyableValue
      label={titleLabel}
      value={detail.entry.title}
      {resetToken}
      {prominent}
      onCopy={(p) => onCopy({ ...p, label: titleLabel })}
    />

    {#if sortedFields.length > 0}
      <div class="section-label">{messages.quickAccess.usefulInformation}</div>
      {#each sortedFields as field (field.id)}
        <CopyableValue
          label={field.key}
          value={field.value}
          sensitive={field.isSensitive}
          {resetToken}
          {prominent}
          onCopy={(p) => onCopy({ ...p, label: field.key })}
        />
      {/each}
    {/if}

    {#if hasNotes}
      <CopyableValue
        label={notesLabel}
        value={detail.entry.notes ?? ''}
        {resetToken}
        {prominent}
        onCopy={(p) => onCopy({ ...p, label: notesLabel })}
      />
    {/if}

    {#each dedupedTags as tag (tag.normalizedTag)}
      <CopyableValue
        label={tag.source === 'ai' ? messages.library.aiTag : messages.library.manualTag}
        value={tag.tag}
        {resetToken}
        {prominent}
        onCopy={(p) => onCopy({ ...p, label: `${tag.source === 'ai' ? messages.library.aiTag : messages.library.manualTag}：${tag.tag}` })}
      />
    {/each}
  {:else}
    <CopyableValue
      label={titleLabel}
      value={detail.entry.title}
      {resetToken}
      onCopy={(p) => onCopy({ ...p, label: titleLabel })}
    />

    {#if hasNotes}
      <CopyableValue
        label={notesLabel}
        value={detail.entry.notes ?? ''}
        {resetToken}
        onCopy={(p) => onCopy({ ...p, label: notesLabel })}
      />
    {/if}

    {#each dedupedTags as tag (tag.normalizedTag)}
      <div class="tag-row">
        <CopyableValue
          label={tag.source === 'ai' ? messages.library.aiTag : messages.library.manualTag}
          value={tag.tag}
          {resetToken}
          onCopy={(p) => onCopy({ ...p, label: `${tag.source === 'ai' ? messages.library.aiTag : messages.library.manualTag}：${tag.tag}` })}
        />
        <span class="source-badge {tag.source}">{tag.source === 'ai' ? 'AI' : 'manual'}</span>
        {#if tag.source === 'ai' && onRemoveAiTag}
          <button
            type="button"
            class="remove-tag-btn"
            aria-label={`${messages.library.delete} ${tag.tag}`}
            title={messages.library.delete}
            onclick={() => onRemoveAiTag?.(tag.normalizedTag)}
          >
            ×
          </button>
        {/if}
      </div>
    {/each}

    {#each sortedFields as field (field.id)}
      <CopyableValue
        label={field.key}
        value={field.value}
        sensitive={field.isSensitive}
        {resetToken}
        onCopy={(p) => onCopy({ ...p, label: field.key })}
      />
    {/each}
  {/if}
</div>

<style>
  .detail {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    padding-top: 0.25rem;
    border-top: 1px solid var(--border-subtle);
    margin-top: 0.25rem;
  }

  .detail.prominent {
    gap: 0.35rem;
    padding: 0.1rem 0.2rem 0.4rem;
    margin-top: 0;
    border-top: 0;
  }

  .section-label {
    margin-top: 0.15rem;
    padding: 0.2rem 0 0.1rem;
    color: var(--color-primary);
    font-size: var(--font-xs, 11px);
    font-weight: 700;
    letter-spacing: 0.04em;
  }

  .tag-row {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  .source-badge {
    font-size: 0.55rem;
    padding: 0.05rem 0.3rem;
    border-radius: var(--radius-md, 0.2rem);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--text-muted);
    background: var(--surface-2);
    flex-shrink: 0;
  }

  .source-badge.ai {
    color: var(--color-primary, #6c8cff);
    background: color-mix(in srgb, var(--color-primary, #6c8cff) 12%, transparent);
  }

  .remove-tag-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0 0.25rem;
    font-size: 0.85rem;
    line-height: 1;
    border-radius: var(--radius-md, 0.2rem);
    font-family: inherit;
  }

  .remove-tag-btn:hover {
    color: #ff6b6b;
    background: color-mix(in srgb, #ff6b6b 15%, transparent);
  }
</style>
