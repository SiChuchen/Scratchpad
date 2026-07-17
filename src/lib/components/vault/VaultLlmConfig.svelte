<script lang="ts">
  import { onMount } from 'svelte'
  import { vaultApi } from '$lib/api/vault'
  import { messages } from '$lib/i18n'
  import type { LlmTestResult, ProviderPreset, VaultAiSettings } from '$lib/types/vault'

  // 已保存的 LLM 配置概览（不含 API Key）
  let savedConfig = $state<{
    providerId: string
    baseUrl: string
    model: string
    hasApiKey: boolean
  } | null>(null)

  // 用户输入；API Key 留空时表示"保持不变"
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

  let presets = $state<ProviderPreset[]>([])
  let aiSettings = $state<VaultAiSettings>({
    autoEnrich: false,
    autoHybridSearch: false,
    sensitiveClipboardClearSeconds: null,
  })

  let showAdvanced = $state(false)
  let testing = $state(false)
  let testResult = $state<LlmTestResult | null>(null)
  let deleteConfirmOpen = $state(false)
  let errorMsg = $state('')
  // 首次保存成功的标志 — 用于决定是否在保存成功后自动启用两项能力
  let wasUnconfigured = $state(false)

  onMount(async () => {
    presets = await vaultApi.getLlmPresets()
    const saved = await vaultApi.getLlmConfig()
    if (saved) {
      savedConfig = saved
      config.providerId = saved.providerId
      config.baseUrl = saved.baseUrl
      config.model = saved.model
      config.apiKey = '' // 输入框始终为空；placeholder 提示
    } else {
      wasUnconfigured = true
    }
    aiSettings = await vaultApi.getAiSettings()
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

  async function saveAndVerify() {
    testing = true
    testResult = null
    errorMsg = ''
    try {
      const input = {
        providerId: config.providerId,
        baseUrl: config.baseUrl,
        // 空字符串传给后端，表示复用已存的 key（仅 provider 未变时合法）
        apiKey: config.apiKey || null,
        model: config.model,
      }
      const result = await vaultApi.verifyAndSaveLlm(input)
      testResult = result
      if (result.ok) {
        const fresh = await vaultApi.getLlmConfig()
        if (fresh) {
          savedConfig = fresh
          config.apiKey = ''
        }
        // 首次配置成功：由后端启用 autoEnrich + autoHybridSearch
        // （后端在 verify_and_save 内部处理；这里只是同步前端 UI）
        if (wasUnconfigured) {
          const refreshed = await vaultApi.getAiSettings()
          aiSettings = refreshed
          wasUnconfigured = false
        }
      }
    } catch (e) {
      errorMsg = String(e)
      testResult = { ok: false, message: String(e), modelEcho: null }
    } finally {
      testing = false
    }
  }

  async function retest() {
    testing = true
    testResult = null
    errorMsg = ''
    try {
      // 用已保存配置测试；不覆盖用户开关
      testResult = await vaultApi.testSavedLlm()
    } catch (e) {
      errorMsg = String(e)
      testResult = { ok: false, message: String(e), modelEcho: null }
    } finally {
      testing = false
    }
  }

  async function requestDelete() {
    deleteConfirmOpen = true
  }

  async function cancelDelete() {
    deleteConfirmOpen = false
  }

  async function confirmDelete() {
    try {
      await vaultApi.deleteLlmConfig()
      savedConfig = null
      config.apiKey = ''
      wasUnconfigured = true
      testResult = null
      deleteConfirmOpen = false
    } catch (e) {
      errorMsg = String(e)
    }
  }

  async function toggleAutoEnrich() {
    const next = { ...aiSettings, autoEnrich: !aiSettings.autoEnrich }
    aiSettings = await vaultApi.setAiSettings(next)
  }

  async function toggleAutoHybridSearch() {
    const next = { ...aiSettings, autoHybridSearch: !aiSettings.autoHybridSearch }
    aiSettings = await vaultApi.setAiSettings(next)
  }

  async function toggleClipboardClear() {
    // 开启写 Some(30)；关闭写 None。不提供任意秒数输入。
    const nextSeconds = aiSettings.sensitiveClipboardClearSeconds === null ? 30 : null
    const next = { ...aiSettings, sensitiveClipboardClearSeconds: nextSeconds }
    aiSettings = await vaultApi.setAiSettings(next)
  }

  const connectionOk = $derived(!!savedConfig?.hasApiKey)
  const clipboardClearOn = $derived(aiSettings.sensitiveClipboardClearSeconds !== null)
  const apiKeyPlaceholder = $derived(savedConfig?.hasApiKey ? messages.aiSettings.savedKeyPlaceholder : 'sk-...')

  // Locale-aware labels for the small bits not covered by aiSettings keys.
  const isZh = $derived(messages.nav.home === '收纳')
  const connectedLabel = $derived(isZh ? '已连接' : 'Connected')
  const unconfiguredLabel = $derived(isZh ? '未配置' : 'Not configured')
  const verifyingLabel = $derived(isZh ? '验证中...' : 'Verifying...')
  const deleteConfirmText = $derived(isZh ? '确认删除 LLM 配置？此操作不可撤销。' : 'Delete LLM configuration? This cannot be undone.')
  const deleteConfirmActionLabel = $derived(isZh ? '确认删除' : 'Confirm delete')
</script>

<div class="llm-config">
  <div class="section-label">{messages.aiSettings.title}</div>

  <!-- 连接状态 -->
  <div class="row">
    <span class="label">{messages.aiSettings.status}</span>
    <span class="status" class:ok={connectionOk} class:fail={!connectionOk} aria-live="polite">
      {connectionOk ? connectedLabel : unconfiguredLabel}
    </span>
  </div>

  <!-- 自动整理与标签 -->
  <div class="row">
    <span class="label">{messages.aiSettings.autoEnrich}</span>
    <button
      type="button"
      class="toggle"
      class:active={aiSettings.autoEnrich}
      onclick={toggleAutoEnrich}
      role="switch"
      aria-checked={aiSettings.autoEnrich}
      aria-label={messages.aiSettings.autoEnrich}
    >
      <div class="toggle-knob"></div>
    </button>
  </div>

  <!-- 自动混合检索 -->
  <div class="row">
    <span class="label">{messages.aiSettings.autoSearch}</span>
    <button
      type="button"
      class="toggle"
      class:active={aiSettings.autoHybridSearch}
      onclick={toggleAutoHybridSearch}
      role="switch"
      aria-checked={aiSettings.autoHybridSearch}
      aria-label={messages.aiSettings.autoSearch}
    >
      <div class="toggle-knob"></div>
    </button>
  </div>

  <!-- 供应商 -->
  <label class="field">
    <span class="label">{messages.aiSettings.provider}</span>
    <select class="select" value={config.providerId} onchange={e => pickProvider(e.currentTarget.value)}>
      {#each presets as p}
        <option value={p.id}>{p.label}</option>
      {/each}
    </select>
  </label>

  <!-- API Key -->
  <label class="field">
    <span class="label">{messages.aiSettings.apiKey}</span>
    <input class="input" type="password" bind:value={config.apiKey} placeholder={apiKeyPlaceholder} autocomplete="off" />
  </label>

  <!-- 操作按钮 -->
  <div class="actions">
    <button class="btn-submit" onclick={saveAndVerify} disabled={testing}>
      {testing ? verifyingLabel : messages.aiSettings.saveAndVerify}
    </button>
    <button class="btn-secondary" onclick={retest} disabled={testing || !connectionOk}>
      {messages.aiSettings.retest}
    </button>
    <button class="btn-danger" onclick={requestDelete} disabled={!savedConfig}>
      {messages.aiSettings.deleteConfig}
    </button>
  </div>

  {#if deleteConfirmOpen}
    <div class="delete-confirm" role="alertdialog" aria-label={messages.aiSettings.deleteConfig}>
      <span class="confirm-text">{deleteConfirmText}</span>
      <div class="confirm-actions">
        <button class="btn-danger" onclick={confirmDelete}>{deleteConfirmActionLabel}</button>
        <button class="btn-secondary" onclick={cancelDelete}>{messages.home.cancel}</button>
      </div>
    </div>
  {/if}

  {#if testResult}
    <div class="test-result" class:ok={testResult.ok} class:fail={!testResult.ok} aria-live="polite">
      <span class="test-icon" aria-hidden="true">{testResult.ok ? '✓' : '✗'}</span>
      <span class="test-msg">{testResult.message}</span>
    </div>
  {/if}

  {#if errorMsg && !testResult}
    <div class="test-result fail" aria-live="polite">
      <span class="test-icon" aria-hidden="true">✗</span>
      <span class="test-msg">{errorMsg}</span>
    </div>
  {/if}

  <!-- 高级（折叠） -->
  <button class="advanced-toggle" onclick={() => showAdvanced = !showAdvanced} aria-expanded={showAdvanced}>
    <span class="chevron" aria-hidden="true">{showAdvanced ? '▾' : '▸'}</span>
    <span>{messages.aiSettings.advanced}</span>
  </button>

  {#if showAdvanced}
    <label class="field">
      <span class="label">{messages.aiSettings.model}</span>
      <input class="input" list="vault-models" bind:value={config.model} />
      <datalist id="vault-models">
        {#each presets.find(p => p.id === config.providerId)?.models ?? [] as m}
          <option value={m}></option>
        {/each}
      </datalist>
    </label>

    <label class="field">
      <span class="label">{messages.aiSettings.baseUrl}</span>
      <input class="input" bind:value={config.baseUrl} placeholder="https://..." />
    </label>
  {/if}

  <!-- 30s clipboard toggle -->
  <div class="row clipboard-row">
    <span class="label">{messages.aiSettings.clipboardClear}</span>
    <button
      type="button"
      class="toggle"
      class:active={clipboardClearOn}
      onclick={toggleClipboardClear}
      role="switch"
      aria-checked={clipboardClearOn}
      aria-label={messages.aiSettings.clipboardClear}
    >
      <div class="toggle-knob"></div>
    </button>
  </div>

  <!-- 数据说明 -->
  <div class="data-notice">
    <div class="notice-title">{messages.aiSettings.title}</div>
    <ul class="notice-list">
      <li>{messages.aiSettings.sendCapture}.</li>
      <li>{messages.aiSettings.sendSearch}.</li>
      <li>{messages.aiSettings.noSensitiveOriginal}.</li>
      <li>{messages.aiSettings.localKey}.</li>
      <li>{messages.aiSettings.noEncryption}.</li>
    </ul>
  </div>
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

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.15rem 0;
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

  .status {
    font-size: var(--font-sm, 0.6rem);
    padding: 0.1rem 0.4rem;
    border-radius: var(--radius-sm, 0.25rem);
  }
  .status.ok {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 12%, transparent);
  }
  .status.fail {
    color: var(--text-muted);
    background: var(--surface-2);
  }

  .toggle {
    width: 2rem;
    height: 1.1rem;
    background: var(--border-default);
    border: none;
    border-radius: 0.55rem;
    position: relative;
    cursor: pointer;
    transition: background 0.2s;
    flex-shrink: 0;
    padding: 0;
  }
  .toggle.active {
    background: var(--color-primary-faint);
  }
  .toggle-knob {
    width: 0.85rem;
    height: 0.85rem;
    background: var(--text-muted);
    border-radius: 50%;
    position: absolute;
    top: 0.125rem;
    left: 0.125rem;
    transition: transform 0.2s, background 0.2s;
  }
  .toggle.active .toggle-knob {
    transform: translateX(0.9rem);
    background: var(--color-primary);
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
    flex-wrap: wrap;
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
  .btn-submit:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-primary) 25%, transparent);
  }
  .btn-submit:disabled {
    opacity: 0.5;
    cursor: not-allowed;
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

  .btn-danger {
    padding: 0.25rem 0.7rem;
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    color: var(--color-danger);
    font-size: var(--font-sm, 0.65rem);
    border-radius: var(--radius-md, 0.3rem);
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s;
  }
  .btn-danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-danger) 20%, transparent);
  }
  .btn-danger:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .delete-confirm {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.4rem 0.5rem;
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    border-radius: var(--radius-md, 0.3rem);
    background: color-mix(in srgb, var(--color-danger) 5%, transparent);
  }
  .confirm-text {
    font-size: var(--font-sm, 0.6rem);
    color: var(--color-danger);
  }
  .confirm-actions {
    display: flex;
    gap: 0.3rem;
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

  .clipboard-row {
    margin-top: 0.2rem;
    padding-top: 0.2rem;
    border-top: 1px solid var(--border-subtle);
  }

  .data-notice {
    margin-top: 0.2rem;
    padding: 0.4rem 0.5rem;
    background: var(--surface-2);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md, 0.3rem);
  }
  .notice-title {
    font-size: var(--font-sm, 0.6rem);
    font-weight: 600;
    color: var(--text-muted);
    margin-bottom: 0.2rem;
  }
  .notice-list {
    margin: 0;
    padding-left: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }
  .notice-list li {
    font-size: var(--font-xs, 0.55rem);
    color: var(--text-muted);
    line-height: 1.4;
  }
</style>
