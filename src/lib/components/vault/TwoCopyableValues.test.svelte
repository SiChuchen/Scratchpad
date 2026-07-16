<script lang="ts">
  import CopyableValue from './CopyableValue.svelte'
  let {
    onCopy,
  }: {
    onCopy: (payload: {
      label: string
      value: string
      sensitive: boolean
    }) => void
  } = $props()
</script>

<CopyableValue label="密码" value="secret-a" sensitive {onCopy} />
<CopyableValue label="Token" value="secret-b" sensitive {onCopy} />
