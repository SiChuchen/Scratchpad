<script lang="ts">
  import { onMount } from 'svelte'
  import type { DockPreferences } from '$lib/types/dock'
  import { dockApi } from '$lib/api/dock'
  import { THEME_PRESETS } from '$lib/themes/presets'
  import { TOKEN_SCHEMA, validateToken } from '$lib/themes/token-schema'

  interface Props {
    preferences: DockPreferences
    onChange: (next: DockPreferences) => void
    onBack: () => void
  }

  let { preferences, onChange, onBack }: Props = $props()

  let installedFonts = $state<string[]>([])
  let zhFontQuery = $state('')
  let enFontQuery = $state('')
  let zhFontOpen = $state(false)
  let enFontOpen = $state(false)
  let zhFontDirty = $state(false)
  let enFontDirty = $state(false)

  let proxyIp = $state('')
  let proxyPort = $state('')
  let appVersion = $state('')
  let updateStatus = $state<'idle' | 'checking' | 'up-to-date' | 'available' | 'downloading' | 'error'>('idle')
  let updateErrorMsg = $state('')
  let pendingUpdate: any = $state(null)

  // Section open/close state
  let themeOpen = $state(true)
  let fontOpen = $state(true)
  let updateOpen = $state(true)
  let advancedOpen = $state(true)
  let expertMode = $state(false)
  let expertErrors = $state<Record<string, string>>({})

  function handleExpertOverride(key: string, value: string) {
    const result = validateToken(key, value)
    if (!result.valid) {
      expertErrors = { ...expertErrors, [key]: result.error || '无效' }
      return
    }
    expertErrors = { ...expertErrors, [key]: '' }
    const overrides = { ...preferences.themeOverrides }
    if (value.trim()) {
      overrides[key] = value
    } else {
      delete overrides[key]
    }
    update({
      themeMode: 'custom',
      customBasePresetId: preferences.themePresetId || 'dark-glass',
      themeOverrides: overrides,
    })
  }

  onMount(async () => {
    try {
      installedFonts = await dockApi.listFonts()
    } catch {
      installedFonts = []
    }
    try {
      const { getVersion } = await import('@tauri-apps/api/app')
      appVersion = await getVersion()
    } catch {}
    // Parse proxy into IP + port
    parseProxy(preferences.updateProxy)
  })

  function parseProxy(proxy: string) {
    if (!proxy) { proxyIp = ''; proxyPort = ''; return }
    const parts = proxy.split(':')
    proxyIp = parts[0] || ''
    proxyPort = parts.slice(1).join(':') || ''
  }

  function update(partial: Partial<DockPreferences>) {
    onChange({ ...preferences, ...partial })
  }

  function selectPreset(id: string) {
    update({
      themeMode: 'preset',
      themePresetId: id,
      customBasePresetId: '',
      themeOverrides: {},
    })
  }

  function toggleThemeAuto() {
    const nextAuto = preferences.themeMode !== 'system'
    update({
      themeMode: nextAuto ? 'system' : 'preset',
    })
  }

  function saveProxy() {
    const ip = proxyIp.trim()
    const port = proxyPort.trim()
    if (ip && port) {
      update({ updateProxy: `${ip}:${port}` })
    } else if (ip) {
      update({ updateProxy: ip })
    } else {
      update({ updateProxy: '' })
    }
  }

  function clearProxy() {
    proxyIp = ''
    proxyPort = ''
    update({ updateProxy: '' })
  }

  function getProxyUrl(): string | undefined {
    const p = preferences.updateProxy.trim()
    if (!p) return undefined
    return p.startsWith('http') ? p : `http://${p}`
  }

  function filteredFonts(query: string): string[] {
    if (!query) return installedFonts.slice(0, 80)
    const q = query.toLowerCase()
    return installedFonts.filter(f => f.toLowerCase().includes(q)).slice(0, 80)
  }

  function pickFont(key: 'fontFamilyZh' | 'fontFamilyEn', value: string) {
    update({ [key]: value })
    if (key === 'fontFamilyZh') { zhFontQuery = ''; zhFontDirty = false; zhFontOpen = false }
    else { enFontQuery = ''; enFontDirty = false; enFontOpen = false }
  }

  function handleReset() {
    const defaultZh = installedFonts.find(f => f === 'Microsoft YaHei') || 'Microsoft YaHei'
    const defaultEn = installedFonts.find(f => f === 'Segoe UI') || 'Segoe UI'
    onChange({
      themeMode: 'system',
      themePresetId: 'dark-glass',
      customBasePresetId: '',
      themeOverrides: {},
      uiTextSizePx: 12,
      contentTextSizePx: 14,
      spacingPreset: 'normal',
      radiusPreset: 'normal',
      dockPositionX: preferences.dockPositionX,
      dockPositionY: preferences.dockPositionY,
      dockWidth: preferences.dockWidth,
      dockHeight: preferences.dockHeight,
      dockEdgeAnchor: 'right',
      dockMinimized: false,
      fontFamilyZh: defaultZh,
      fontFamilyEn: defaultEn,
      launchOnStartup: false,
      updateProxy: '',
    })
    proxyIp = ''
    proxyPort = ''
  }

  async function handleCheckUpdate() {
    updateStatus = 'checking'
    updateErrorMsg = ''
    try {
      const { check } = await import('@tauri-apps/plugin-updater')
      const result = await check({ proxy: getProxyUrl() })
      if (result?.available) {
        pendingUpdate = result
        updateStatus = 'available'
      } else {
        updateStatus = 'up-to-date'
      }
    } catch (e) {
      updateStatus = 'error'
      updateErrorMsg = e instanceof Error ? e.message : '检查失败'
    }
  }

  async function handleInstallUpdate() {
    if (!pendingUpdate) return
    updateStatus = 'downloading'
    try {
      await pendingUpdate.downloadAndInstall(undefined, { proxy: getProxyUrl() })
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      getCurrentWindow().close()
    } catch (e) {
      updateStatus = 'error'
      updateErrorMsg = e instanceof Error ? e.message : '更新失败'
    }
  }

  const themeAuto = $derived(preferences.themeMode === 'system')
</script>

<div class="settings-view">
  <div class="settings-header">
    <button class="back-btn" onclick={onBack}>← 返回</button>
  </div>

  <div class="settings-body">
    <!-- Theme section -->
    <div class="section">
      <div class="section-header" onclick={() => themeOpen = !themeOpen}>
        <span class="section-label">主题</span>
        <span class="chevron" class:open={themeOpen}>▾</span>
      </div>
      {#if themeOpen}
        <div class="section-body">
          <div class="row">
            <span class="label">跟随系统</span>
            <div class="toggle" class:active={themeAuto} onclick={toggleThemeAuto}>
              <div class="toggle-knob"></div>
            </div>
          </div>
          {#if !themeAuto}
            <div class="theme-cards">
              {#each Object.values(THEME_PRESETS) as preset}
                <button
                  class="theme-card"
                  class:active={preferences.themePresetId === preset.id}
                  onclick={() => selectPreset(preset.id)}
                >
                  <div class="swatch" style="background:{preset.tokens['--surface-0']}"></div>
                  <span class="card-name">{preset.name}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <!-- Font section -->
    <div class="section">
      <div class="section-header" onclick={() => fontOpen = !fontOpen}>
        <span class="section-label">字体</span>
        <span class="chevron" class:open={fontOpen}>▾</span>
      </div>
      {#if fontOpen}
        <div class="section-body">
          <div class="row">
            <span class="label">中文字体</span>
            <div class="font-picker">
              <input
                type="text"
                class="font-input"
                placeholder="搜索中文字体..."
                value={zhFontDirty ? zhFontQuery : preferences.fontFamilyZh}
                onfocus={() => { zhFontQuery = ''; zhFontDirty = false; zhFontOpen = true }}
                onblur={() => setTimeout(() => { zhFontOpen = false }, 150)}
                oninput={(e) => { zhFontQuery = (e.target as HTMLInputElement).value; zhFontDirty = true }}
              />
              {#if zhFontOpen}
                <ul class="font-list">
                  {#each filteredFonts(zhFontDirty ? zhFontQuery : '') as font}
                    <li
                      class="font-item"
                      class:active={font === preferences.fontFamilyZh}
                      onmousedown={() => pickFont('fontFamilyZh', font)}
                    >{font}</li>
                  {/each}
                </ul>
              {/if}
            </div>
          </div>
          <div class="row">
            <span class="label">英文字体</span>
            <div class="font-picker">
              <input
                type="text"
                class="font-input"
                placeholder="搜索英文字体..."
                value={enFontDirty ? enFontQuery : preferences.fontFamilyEn}
                onfocus={() => { enFontQuery = ''; enFontDirty = false; enFontOpen = true }}
                onblur={() => setTimeout(() => { enFontOpen = false }, 150)}
                oninput={(e) => { enFontQuery = (e.target as HTMLInputElement).value; enFontDirty = true }}
              />
              {#if enFontOpen}
                <ul class="font-list">
                  {#each filteredFonts(enFontDirty ? enFontQuery : '') as font}
                    <li
                      class="font-item"
                      class:active={font === preferences.fontFamilyEn}
                      onmousedown={() => pickFont('fontFamilyEn', font)}
                    >{font}</li>
                  {/each}
                </ul>
              {/if}
            </div>
          </div>
        </div>
      {/if}
    </div>

    <!-- Update section -->
    <div class="section">
      <div class="section-header" onclick={() => updateOpen = !updateOpen}>
        <span class="section-label">更新</span>
        <span class="chevron" class:open={updateOpen}>▾</span>
      </div>
      {#if updateOpen}
        <div class="section-body">
          <div class="row proxy-row">
            <span class="label">代理地址</span>
            <div class="proxy-fields">
              <input
                type="text"
                class="proxy-input"
                placeholder="IP 地址"
                bind:value={proxyIp}
              />
              <span class="proxy-colon">:</span>
              <input
                type="text"
                class="proxy-input proxy-port"
                placeholder="端口"
                bind:value={proxyPort}
              />
            </div>
          </div>
          <div class="row proxy-actions">
            <span class="label"></span>
            <div class="proxy-btns">
              <button class="proxy-save-btn" onclick={saveProxy}>保存</button>
              <button class="proxy-clear-btn" onclick={clearProxy}>清除</button>
            </div>
          </div>
          <div class="about-section">
            <div class="about-header">
              <span class="about-title">Soma Scratchpad</span>
              {#if appVersion}
                <span class="about-version">v{appVersion}</span>
              {/if}
            </div>
            <div class="update-row">
              {#if updateStatus === 'idle'}
                <button class="update-btn" onclick={handleCheckUpdate}>检查更新</button>
              {:else if updateStatus === 'checking'}
                <span class="update-status">正在检查...</span>
              {:else if updateStatus === 'up-to-date'}
                <span class="update-status ok">已是最新版本</span>
                <button class="update-btn" onclick={() => { updateStatus = 'idle' }}>重新检查</button>
              {:else if updateStatus === 'available'}
                <span class="update-status available">发现新版本</span>
                <button class="update-btn install-btn" onclick={handleInstallUpdate}>立即更新</button>
              {:else if updateStatus === 'downloading'}
                <span class="update-status">正在下载并安装...</span>
              {:else if updateStatus === 'error'}
                <span class="update-status err">{updateErrorMsg}</span>
                <button class="update-btn" onclick={handleCheckUpdate}>重试</button>
              {/if}
            </div>
          </div>
        </div>
      {/if}
    </div>

    <!-- Advanced section -->
    <div class="section">
      <div class="section-header" onclick={() => advancedOpen = !advancedOpen}>
        <span class="section-label">高级</span>
        <span class="chevron" class:open={advancedOpen}>▾</span>
      </div>
      {#if advancedOpen}
        <div class="section-body">
          <div class="row">
            <span class="label">开机自启</span>
            <div class="toggle" class:active={preferences.launchOnStartup}
                 onclick={() => update({ launchOnStartup: !preferences.launchOnStartup })}>
              <div class="toggle-knob"></div>
            </div>
          </div>
          <div class="row">
            <span class="label">专家模式</span>
            <div class="toggle" class:active={expertMode} onclick={() => expertMode = !expertMode}>
              <div class="toggle-knob"></div>
            </div>
          </div>
          {#if expertMode}
            <div class="expert-list">
              {#each Object.entries(TOKEN_SCHEMA) as [key, schema]}
                {@const current = preferences.themeOverrides[key] || ''}
                <div class="expert-row">
                  <span class="expert-label">{schema.label}</span>
                  <input
                    class="expert-input"
                    class:invalid={expertErrors[key]}
                    type="text"
                    value={current}
                    placeholder="使用预设值"
                    onblur={(e) => handleExpertOverride(key, (e.target as HTMLInputElement).value)}
                    onkeydown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur() }}
                  />
                  {#if expertErrors[key]}
                    <span class="expert-error">{expertErrors[key]}</span>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
          <div class="row" style="margin-top: 0.5rem;">
            <button class="reset-btn" onclick={handleReset}>重置全部设置</button>
          </div>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .settings-view {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 0.5rem 0.65rem;
    overflow: hidden;
    min-height: 0;
  }

  .settings-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex-shrink: 0;
    padding-bottom: 0.4rem;
    border-bottom: 1px solid var(--border-subtle);
  }

  .back-btn {
    background: none;
    border: 1px solid transparent;
    color: color-mix(in srgb, var(--text-primary) 60%, transparent);
    font-size: var(--font-sm, 0.65rem);
    cursor: pointer;
    padding: 0.2rem 0.35rem;
    border-radius: var(--radius-sm, 0.25rem);
    font-family: inherit;
    transition: color 0.12s, background 0.12s;
  }

  .back-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
  }

  .settings-title {
    font-size: var(--font-sm, 0.7rem);
    font-weight: 700;
    color: var(--text-muted);
  }

  .settings-body {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    padding-top: 0.5rem;
  }

  /* Sections */
  .section {
    border-bottom: 1px solid var(--border-subtle);
    padding-bottom: 0.3rem;
  }

  .section:last-child {
    border-bottom: none;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.3rem 0;
    cursor: pointer;
    user-select: none;
  }

  .section-label {
    font-size: var(--font-sm, 0.65rem);
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .chevron {
    font-size: var(--font-sm, 0.65rem);
    color: var(--text-faint);
    transition: transform 0.15s;
  }

  .chevron.open {
    transform: rotate(180deg);
  }

  .section-body {
    padding-bottom: 0.25rem;
  }

  /* Rows */
  .row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.2rem 0;
    font-size: var(--font-sm, 0.65rem);
    color: var(--text-primary);
  }

  .label {
    min-width: 4.5rem;
    color: var(--text-muted);
    font-size: var(--font-sm, 0.65rem);
  }

  /* Toggle */
  .toggle {
    width: 2rem;
    height: 1.1rem;
    background: var(--border-default);
    border-radius: 0.55rem;
    position: relative;
    cursor: pointer;
    transition: background 0.2s;
    flex-shrink: 0;
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

  /* Theme cards */
  .theme-cards {
    display: flex;
    gap: 0.4rem;
    padding: 0.3rem 0;
  }

  .theme-card {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.25rem;
    padding: 0.4rem;
    border: 2px solid var(--border-default);
    border-radius: var(--radius-md, 0.35rem);
    background: transparent;
    cursor: pointer;
    transition: border-color 0.15s, background 0.15s;
    font-family: inherit;
  }

  .theme-card.active {
    border-color: var(--color-primary);
    background: var(--color-primary-faint);
  }

  .theme-card:hover:not(.active) {
    border-color: var(--border-emphasis);
  }

  .swatch {
    width: 100%;
    height: 1.2rem;
    border-radius: 0.15rem;
    border: 1px solid var(--border-default);
  }

  .card-name {
    font-size: var(--font-xs, 0.55rem);
    color: var(--text-primary);
    white-space: nowrap;
  }

  /* Font picker */
  .font-picker {
    flex: 1;
    position: relative;
  }

  .font-input {
    width: 100%;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm, 0.25rem);
    color: var(--text-primary);
    font-size: var(--font-sm, 0.6rem);
    padding: 0.2rem 0.3rem;
    font-family: inherit;
  }

  .font-input:focus {
    outline: none;
    border-color: var(--color-primary-light);
  }

  .font-input::placeholder {
    color: var(--text-faint);
  }

  .font-list {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    max-height: 8rem;
    overflow-y: auto;
    margin: 0;
    padding: 0;
    list-style: none;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-top: none;
    border-radius: 0 0 var(--radius-sm, 0.25rem) var(--radius-sm, 0.25rem);
    z-index: 50;
  }

  .font-item {
    padding: 0.2rem 0.4rem;
    font-size: var(--font-xs, 0.55rem);
    color: var(--text-primary);
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .font-item:hover {
    background: var(--color-primary-faint);
    color: var(--color-primary);
  }

  .font-item.active {
    background: var(--color-primary-faint);
    color: var(--color-primary);
  }

  /* Proxy */
  .proxy-row {
    align-items: flex-start;
  }

  .proxy-fields {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.2rem;
  }

  .proxy-input {
    flex: 1;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm, 0.25rem);
    color: var(--text-primary);
    font-size: var(--font-sm, 0.6rem);
    padding: 0.2rem 0.3rem;
    font-family: inherit;
    min-width: 0;
  }

  .proxy-port {
    max-width: 4rem;
  }

  .proxy-input:focus {
    outline: none;
    border-color: var(--color-primary-light);
  }

  .proxy-input::placeholder {
    color: var(--text-faint);
  }

  .proxy-colon {
    color: var(--text-muted);
    font-size: var(--font-sm, 0.6rem);
  }

  .proxy-actions {
    justify-content: flex-end;
  }

  .proxy-btns {
    display: flex;
    gap: 0.3rem;
    margin-left: calc(4.5rem + 0.5rem);
  }

  .proxy-save-btn,
  .proxy-clear-btn {
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    color: var(--text-muted);
    font-size: var(--font-sm, 0.6rem);
    padding: 0.2rem 0.5rem;
    border-radius: var(--radius-sm, 0.25rem);
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s, color 0.12s;
  }

  .proxy-save-btn:hover,
  .proxy-clear-btn:hover {
    background: var(--border-default);
    color: var(--text-primary);
  }

  /* About / Update */
  .about-section {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.3rem 0;
  }

  .about-header {
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
  }

  .about-title {
    font-size: var(--font-sm, 0.7rem);
    font-weight: 600;
    color: var(--text-primary);
  }

  .about-version {
    font-size: var(--font-xs, 0.6rem);
    color: var(--text-faint);
  }

  .update-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .update-status {
    font-size: var(--font-xs, 0.6rem);
    color: var(--text-muted);
  }

  .update-status.ok {
    color: var(--color-success);
  }

  .update-status.available {
    color: var(--color-primary);
    font-weight: 500;
  }

  .update-status.err {
    color: var(--color-danger);
  }

  .update-btn {
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    color: var(--text-muted);
    font-size: var(--font-sm, 0.6rem);
    padding: 0.2rem 0.6rem;
    border-radius: var(--radius-sm, 0.25rem);
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s, color 0.12s, border-color 0.12s;
  }

  .update-btn:hover {
    background: var(--border-default);
    color: var(--text-primary);
    border-color: var(--border-emphasis);
  }

  .install-btn {
    background: var(--color-primary-faint);
    border-color: var(--color-primary-light);
    color: var(--color-primary);
    font-weight: 500;
  }

  .install-btn:hover {
    background: var(--color-primary-light);
  }

  /* Reset */
  .reset-btn {
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    color: var(--color-danger);
    font-size: var(--font-sm, 0.6rem);
    padding: 0.25rem 0.75rem;
    border-radius: var(--radius-sm, 0.3rem);
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s, border-color 0.12s;
  }

  .reset-btn:hover {
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-danger) 30%, transparent);
  }

  /* Expert mode */
  .expert-list {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    max-height: 12rem;
    overflow-y: auto;
    padding: 0.3rem 0;
    border-top: 1px solid var(--border-subtle);
    border-bottom: 1px solid var(--border-subtle);
    margin: 0.3rem 0;
  }

  .expert-row {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    font-size: var(--font-xs, 0.55rem);
  }

  .expert-label {
    min-width: 4.5rem;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .expert-input {
    flex: 1;
    min-width: 0;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm, 0.2rem);
    color: var(--text-primary);
    font-size: var(--font-xs, 0.55rem);
    padding: 0.15rem 0.3rem;
    font-family: 'Cascadia Code', 'Consolas', monospace;
    outline: none;
  }

  .expert-input:focus {
    border-color: var(--color-primary-light);
  }

  .expert-input.invalid {
    border-color: var(--color-danger);
  }

  .expert-error {
    font-size: var(--font-xs, 0.5rem);
    color: var(--color-danger);
    flex-shrink: 0;
  }
</style>
