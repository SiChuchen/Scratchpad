<script lang="ts" module>
  // Central SVG icon registry — single source for all UI icons.
  // Every icon is normalized to path data on a 24×24 viewBox so the
  // component stays tiny and tree-shake friendly.
  const ICONS: Record<string, { d: string[]; fill?: boolean }> = {
    // Content kinds
    text: {
      d: [
        'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8l-6-6z',
        'M14 2v6h6M16 13H8M16 17H8M10 9H8',
      ],
    },
    image: {
      d: [
        'M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z',
        'M8.5 7a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3z',
        'M21 15l-5-5L5 21',
      ],
    },
    file: {
      d: [
        'M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9l-7-7z',
        'M13 2v7h7',
      ],
    },
    credential: {
      d: [
        'M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4',
      ],
    },
    bookmark: {
      d: ['M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z'],
    },
    note: {
      d: ['M12 20h9', 'M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4L16.5 3.5z'],
    },
    // Window / navigation
    pin: {
      d: [
        'M12 17v5',
        'M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1z',
      ],
    },
    minus: { d: ['M5 12h14'] },
    back: { d: ['M19 12H5', 'M12 19l-7-7 7-7'] },
    // Actions
    search: { d: ['M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14z', 'M21 21l-4.35-4.35'] },
    x: { d: ['M18 6L6 18M6 6l12 12'] },
    plus: { d: ['M12 5v14M5 12h14'] },
    copy: {
      d: [
        'M11 9h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-8a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z',
        'M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1',
      ],
    },
    link: {
      d: [
        'M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71',
        'M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71',
      ],
    },
    folder: {
      d: [
        'M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z',
      ],
    },
    star: {
      d: ['M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z'],
    },
    trash: {
      d: ['M3 6h18', 'M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2'],
    },
    'chevron-down': { d: ['M6 9l6 6 6-6'] },
    'chevron-up': { d: ['M18 15l-6-6-6 6'] },
    'external-link': {
      d: ['M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6', 'M15 3h6v6', 'M10 14L21 3'],
    },
    eye: {
      d: ['M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z', 'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z'],
    },
    'eye-off': {
      d: [
        'M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24',
        'M1 1l22 22',
      ],
    },
    check: { d: ['M20 6L9 17l-5-5'] },
    inbox: {
      d: [
        'M22 12h-6l-2 3h-4l-2-3H2',
        'M5.45 5.11L2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z',
      ],
    },
    grip: {
      fill: true,
      d: [
        'M9 5a1.1 1.1 0 1 0 0 2.2A1.1 1.1 0 0 0 9 5zM15 5a1.1 1.1 0 1 0 0 2.2A1.1 1.1 0 0 0 15 5zM9 11a1.1 1.1 0 1 0 0 2.2A1.1 1.1 0 0 0 9 11zM15 11a1.1 1.1 0 1 0 0 2.2A1.1 1.1 0 0 0 15 11zM9 17a1.1 1.1 0 1 0 0 2.2A1.1 1.1 0 0 0 9 17zM15 17a1.1 1.1 0 1 0 0 2.2A1.1 1.1 0 0 0 15 17z',
      ],
    },
  }

  export type IconName = keyof typeof ICONS
</script>

<script lang="ts">
  interface Props {
    name: IconName
    size?: number
    strokeWidth?: number
    filled?: boolean
  }

  let { name, size = 14, strokeWidth = 1.8, filled = false }: Props = $props()

  const icon = $derived(ICONS[name] ?? ICONS.file)
  const useFill = $derived(icon.fill === true || filled)
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={useFill ? 'currentColor' : 'none'}
  stroke={useFill ? 'none' : 'currentColor'}
  stroke-width={useFill ? 0 : strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#each icon.d as d}
    <path {d} />
  {/each}
</svg>
