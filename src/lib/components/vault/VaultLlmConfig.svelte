<script lang="ts">
  import { onMount } from 'svelte'
  import { vaultApi } from '$lib/api/vault'
  import type { LlmTestResult, ProviderPreset } from '$lib/types/vault'

  let presets = $state<ProviderPreset[]>([])
  let config = $state<{
    providerId: string
    baseUrl: string
    apiKey: string
    model: string
  }>({
    providerId: 'deepseek',
    baseUrl: 'https://api.deepseek.com/v1',
    apiKey: '',
    model: 'deepseek-v4-flash',
  })
  let showAdvanced = $state(false)
  let testing = $state(false)
  let testResult = $state<LlmTestResult | null>(null)

  onMount(async () => {
    presets = await vaultApi.getLlmPresets()
    const saved = await vaultApi.getLlmConfig()
    if (saved) {
      config.providerId = saved.providerId
      config.baseUrl = saved.baseUrl
      config.model = saved.model
    }
  })

  function pickProvider(id: string) {
    const p = presets.find(x => x.id === id)
    if (!p) return
    config.providerId = id
    if (!showAdvanced) {
      config.baseUrl = p.baseUrl
      if (p.defaultModel) config.model = p.defaultModel
    }
  }

  async function save() {
    // 临时沿用旧 setLlmConfig 兼容别名；Task 14 会迁移到 verifyAndSaveLlm。
    await vaultApi.setLlmConfig({ ...config })
    testResult = { ok: true, message: '配置已保存', modelEcho: null }
  }

  async function test() {
    testing = true
    testResult = null
    try {
      testResult = await vaultApi.testLlm({ ...config })
    } finally {
      testing = false
    }
  }
</script>

<div class="llm-config">
  <div class="section-label">Vault LLM</div>

  <label class="field">
    <span class="label">厂商</span>
    <select class="select" value={config.providerId} onchange={e => pickProvider(e.currentTarget.value)}>
      {#each presets as p}
        <option value={p.id}>{p.label}</option>
      {/each}
    </select>
  </label>

  <label class="field">
    <span class="label">API Key</span>
    <input class="input" type="password" bind:value={config.apiKey} placeholder="sk-..." />
  </label>

  <label class="field">
    <span class="label">模型</span>
    <input class="input" list="vault-models" bind:value={config.model} />
    <datalist id="vault-models">
      {#each presets.find(p => p.id === config.providerId)?.models ?? [] as m}
        <option value={m}></option>
      {/each}
    </datalist>
  </label>

  <div class="actions">
    <button class="btn-submit" onclick={save}>保存</button>
    <button class="btn-secondary" onclick={test} disabled={testing}>
      {testing ? '测试中...' : '测试连接'}
    </button>
  </div>

  {#if testResult}
    <div class="test-result" class:ok={testResult.ok} class:fail={!testResult.ok}>
      <span class="test-icon">{testResult.ok ? '✓' : '✗'}</span>
      <span class="test-msg">{testResult.message}</span>
    </div>
  {/if}

  <button class="advanced-toggle" onclick={() => showAdvanced = !showAdvanced}>
    <span class="chevron">{showAdvanced ? '▾' : '▸'}</span>
    <span>高级</span>
  </button>

  {#if showAdvanced}
    <label class="field">
      <span class="label">Base URL</span>
      <input class="input" bind:value={config.baseUrl} placeholder="https://..." />
    </label>
  {/if}
</div>

<style>
  .llm-config {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.4rem 0;
  }

  .section-label {
    font-size: var(--font-sm, 0.65rem);
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .label {
    font-size: var(--font-sm, 0.6rem);
    color: var(--text-muted);
    font-weight: 500;
  }

  .input, .select {
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 0.3rem);
    color: var(--text-primary);
    font-size: var(--font-sm, 0.7rem);
    font-family: inherit;
    padding: 0.3rem 0.45rem;
    outline: none;
    transition: border-color 0.12s;
    width: 100%;
  }

  .input::placeholder {
    color: var(--text-faint);
  }

  .input:focus, .select:focus {
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
  }

  .select {
    appearance: none;
    background-image: url("data:image/svg+xml;charset=utf-8,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10' viewBox='0 0 24 24' fill='none' stroke='%2394a3b8' stroke-width='2'%3E%3Cpolyline points='6 9 12 15 18 9'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 0.4rem center;
    padding-right: 1.5rem;
  }

  .actions {
    display: flex;
    gap: 0.3rem;
    margin-top: 0.1rem;
  }

  .btn-submit {
    padding: 0.25rem 0.7rem;
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    color: var(--color-primary);
    font-size: var(--font-sm, 0.65rem);
    font-weight: 500;
    border-radius: var(--radius-md, 0.3rem);
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s;
  }

  .btn-submit:hover {
    background: color-mix(in srgb, var(--color-primary) 25%, transparent);
  }

  .btn-secondary {
    padding: 0.25rem 0.7rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    color: var(--text-muted);
    font-size: var(--font-sm, 0.65rem);
    border-radius: var(--radius-md, 0.3rem);
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s, color 0.12s;
  }

  .btn-secondary:hover:not(:disabled) {
    background: var(--border-default);
    color: var(--text-primary);
  }

  .btn-secondary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .test-result {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.3rem 0.5rem;
    border-radius: var(--radius-md, 0.3rem);
    font-size: var(--font-sm, 0.65rem);
  }

  .test-result.ok {
    background: color-mix(in srgb, #4ade80 12%, transparent);
    color: #4ade80;
  }

  .test-result.fail {
    background: color-mix(in srgb, #ff6b6b 12%, transparent);
    color: #ff6b6b;
  }

  .test-icon {
    font-weight: 700;
  }

  .advanced-toggle {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: var(--font-sm, 0.65rem);
    cursor: pointer;
    padding: 0.2rem 0;
    font-family: inherit;
    transition: color 0.12s;
  }

  .advanced-toggle:hover {
    color: var(--text-primary);
  }

  .chevron {
    width: 0.7rem;
    text-align: center;
  }
</style>
