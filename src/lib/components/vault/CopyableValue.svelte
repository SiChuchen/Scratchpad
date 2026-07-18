<script lang="ts">
  // src/lib/components/vault/CopyableValue.svelte
  //
  // Vault 条目统一字段展示组件。
  //
  // 行为要点：
  //   * 敏感值默认以 `••••••••` 掩码展示，需要点击眼睛按钮才显形；
  //   * 眼睛按钮的状态是每行独立（component 实例本地 state）；
  //   * 复制按钮始终可用，不需要先 reveal；onCopy 回调 payload 直接
  //     包含真实 value；
  //   * window blur 时自动重新掩码（防止切到其他应用时泄密）；
  //   * resetToken 变化（父组件折叠 / 切换条目）时也立即重新掩码。
  //
  // 紧凑模式用于主资料库；prominent 模式用于快速入口的高频复制操作。

  import { onMount } from 'svelte'
  import { messages } from '$lib/i18n'

  interface Props {
    label: string
    value: string
    sensitive?: boolean
    resetToken?: number | string
    prominent?: boolean
    onCopy: (payload: {
      label: string
      value: string
      sensitive: boolean
    }) => void | Promise<void>
  }

  let {
    label,
    value,
    sensitive = false,
    resetToken,
    prominent = false,
    onCopy,
  }: Props = $props()

  let revealed = $state(false)

  // resetToken 变化（含初始挂载）→ 立即掩码
  $effect(() => {
    void resetToken
    revealed = false
  })

  function handleBlur() {
    revealed = false
  }

  onMount(() => {
    window.addEventListener('blur', handleBlur)
    return () => {
      window.removeEventListener('blur', handleBlur)
    }
  })

  function toggleReveal() {
    revealed = !revealed
  }

  async function handleCopy() {
    await onCopy({ label, value, sensitive })
  }

  const displayValue = $derived(
    sensitive && !revealed ? '••••••••' : value,
  )
  const eyeAriaLabel = $derived(
    revealed
      ? messages.library.hideLabel.replace('{label}', label)
      : messages.library.showLabel.replace('{label}', label),
  )
  const copyAriaLabel = $derived(messages.library.copyLabel.replace('{label}', label))
</script>

<div class="copyable-row" class:prominent>
  <span class="label">{label}</span>
  <code class="value">{displayValue}</code>
  <div class="actions" data-testid="copyable-actions">
    {#if sensitive}
      <button
        type="button"
        class="icon-btn"
        onclick={toggleReveal}
        aria-label={eyeAriaLabel}
        title={eyeAriaLabel}
      >
        {#if revealed}
          <svg
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            aria-hidden="true"
          >
            <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"></path>
            <line x1="1" y1="1" x2="23" y2="23"></line>
          </svg>
        {:else}
          <svg
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            aria-hidden="true"
          >
            <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"></path>
            <circle cx="12" cy="12" r="3"></circle>
          </svg>
        {/if}
      </button>
    {/if}
    <button
      type="button"
      class="icon-btn copy-btn"
      class:prominent-action={prominent}
      data-prominent-action={prominent ? 'copy' : undefined}
      onclick={handleCopy}
      aria-label={copyAriaLabel}
      title={copyAriaLabel}
    >
      {#if prominent}
        <span>{messages.entry.copy}</span>
      {:else}
        <svg
          width="12"
          height="12"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          aria-hidden="true"
        >
          <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
        </svg>
      {/if}
    </button>
  </div>
</div>

<style>
  .copyable-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-height: 1.4rem;
  }

  .label {
    width: 5rem;
    flex-shrink: 0;
    color: var(--text-muted);
    font-size: var(--font-sm, 0.65rem);
  }

  .value {
    flex: 1;
    color: var(--text-primary);
    font-size: var(--font-sm, 0.7rem);
    font-family: var(--font-family-mono, 'Cascadia Code', 'Consolas', monospace);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .actions {
    display: flex;
    gap: 0.2rem;
    flex-shrink: 0;
  }

  .icon-btn {
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    color: var(--text-muted);
    padding: 0.2rem;
    border-radius: var(--radius-md, 0.25rem);
    cursor: pointer;
    font-family: inherit;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.12s, background 0.12s, border-color 0.12s;
  }

  .icon-btn:hover {
    color: var(--color-primary);
    border-color: color-mix(in srgb, var(--color-primary) 30%, transparent);
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
  }

  .icon-btn:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 1px;
  }

  .copyable-row.prominent {
    min-height: 2.5rem;
    gap: 0.5rem;
  }

  .copyable-row.prominent .label {
    width: 4.25rem;
    font-size: var(--font-sm, 12px);
  }

  .copyable-row.prominent .value {
    font-size: var(--font-md, 14px);
  }

  .copyable-row.prominent .icon-btn {
    width: 2rem;
    height: 2rem;
    padding: 0;
  }

  .copyable-row.prominent .copy-btn {
    width: auto;
    min-width: 3.25rem;
    padding: 0 0.65rem;
    font-size: var(--font-sm, 12px);
    font-weight: 650;
  }
</style>
