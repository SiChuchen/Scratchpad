<script lang="ts">
  import type { ContentKind } from '$lib/types/content'
  let { kind }: { kind: ContentKind } = $props()
  const glyphs: Record<ContentKind, string> = { text: 'T', image: '▧', file: 'F', credential: 'K', bookmark: '↗', note: 'N' }
</script>
<span class="kind-icon" aria-hidden="true">{glyphs[kind]}</span>

<style>
  .kind-icon { width: 1.4rem; height: 1.4rem; display: inline-grid; place-items: center; flex: 0 0 auto; border-radius: var(--radius-md, .3rem); color: var(--color-primary); background: color-mix(in srgb, var(--color-primary) 13%, var(--surface-1)); font-size: var(--font-xs, .65rem); font-weight: 800; }
</style>
