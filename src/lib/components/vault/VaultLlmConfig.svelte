<script lang="ts">
  import { onMount } from 'svelte'
  import { vaultApi } from '$lib/api/vault'
  import type { LlmConfig, LlmTestResult, ProviderPreset } from '$lib/types/vault'

  let presets = $state<ProviderPreset[]>([])
  let config = $state<LlmConfig>({
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
    if (saved) config = saved
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
    await vaultApi.setLlmConfig({ ...config })
    alert('已保存')
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
  <h3>Vault LLM</h3>

  <label>厂商
    <select value={config.providerId} onchange={e => pickProvider(e.currentTarget.value)}>
      {#each presets as p}
        <option value={p.id}>{p.label}</option>
      {/each}
    </select>
  </label>

  <label>API Key
    <input type="password" bind:value={config.apiKey} placeholder="sk-..." />
  </label>

  <label>模型
    <input list="vault-models" bind:value={config.model} />
    <datalist id="vault-models">
      {#each presets.find(p => p.id === config.providerId)?.models ?? [] as m}
        <option value={m}></option>
      {/each}
    </datalist>
  </label>

  <div class="actions">
    <button onclick={save}>保存</button>
    <button onclick={test} disabled={testing}>{testing ? '测试中...' : '测试连接'}</button>
  </div>

  {#if testResult}
    <div class="test-result" class:ok={testResult.ok} class:fail={!testResult.ok}>
      {testResult.ok ? '✓' : '✗'} {testResult.message}
    </div>
  {/if}

  <button class="advanced-toggle" onclick={() => showAdvanced = !showAdvanced}>
    {showAdvanced ? '▾' : '▸'} 高级
  </button>

  {#if showAdvanced}
    <label>Base URL
      <input bind:value={config.baseUrl} placeholder="https://..." />
    </label>
  {/if}
</div>

<style>
  .llm-config { display: flex; flex-direction: column; gap: 8px; padding: 10px; border-top: 1px solid var(--border-color, #ccc); }
  .llm-config label { display: flex; flex-direction: column; gap: 2px; font-size: 0.85em; }
  .llm-config input, .llm-config select { padding: 4px 6px; }
  .actions { display: flex; gap: 6px; }
  .advanced-toggle { background: none; border: none; cursor: pointer; opacity: 0.6; font-size: 0.85em; text-align: left; }
  .test-result { padding: 4px 8px; border-radius: 4px; font-size: 0.85em; }
  .test-result.ok { background: rgba(0,128,0,0.1); color: #060; }
  .test-result.fail { background: rgba(128,0,0,0.1); color: #600; }
</style>
