<script lang="ts">
  import Icon, { type IconName } from '$lib/components/Icon.svelte'
  import type { ContentKind } from '$lib/types/content'

  let { kind }: { kind: ContentKind } = $props()

  const icons: Record<ContentKind, IconName> = {
    text: 'text',
    image: 'image',
    file: 'file',
    credential: 'credential',
    bookmark: 'bookmark',
    note: 'note',
  }
</script>

<span class="kind-icon" data-kind={kind} aria-hidden="true">
  <Icon name={icons[kind]} size={13} />
</span>

<style>
  .kind-icon {
    width: 1.5rem;
    height: 1.5rem;
    display: inline-grid;
    place-items: center;
    flex: 0 0 auto;
    border-radius: var(--radius-md, 0.3rem);
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 13%, transparent);
  }

  .kind-icon[data-kind='image'] {
    color: var(--color-info);
    background: color-mix(in srgb, var(--color-info) 12%, transparent);
  }

  .kind-icon[data-kind='file'] {
    color: var(--color-file);
    background: color-mix(in srgb, var(--color-file) 12%, transparent);
  }

  .kind-icon[data-kind='credential'] {
    color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
  }

  .kind-icon[data-kind='note'] {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 12%, transparent);
  }
</style>
