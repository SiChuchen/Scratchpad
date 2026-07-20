<script lang="ts">
  import SimpleContentDetail from './SimpleContentDetail.svelte'
  import StructuredContentDetail from './StructuredContentDetail.svelte'
  import { messages } from '$lib/i18n'
  import type { ContentDetail as Detail } from '$lib/types/content'

  interface Props {
    detail: Detail
    resetToken: string | number
    onClose: () => void
    onChanged: (id: string) => Promise<void>
    onNotify: (message: string, kind?: 'success' | 'error') => void
    onDelete?: () => void
    onToggleSaved?: () => void
  }

  let { detail, resetToken, onClose, onChanged, onNotify, onDelete, onToggleSaved }: Props = $props()
</script>

<section class="detail-shell" aria-label={messages.workspace.detailLabel}>
  {#if detail.kind === 'text' || detail.kind === 'image' || detail.kind === 'file'}
    <SimpleContentDetail {detail} {onClose} {onChanged} {onNotify} {onDelete} {onToggleSaved} />
  {:else}
    <StructuredContentDetail {detail} {resetToken} {onClose} {onChanged} {onNotify} {onDelete} {onToggleSaved} />
  {/if}
</section>

<style>
  .detail-shell {
    height: 100%;
    min-width: 0;
    overflow: auto;
    background: var(--surface-0);
  }
</style>
