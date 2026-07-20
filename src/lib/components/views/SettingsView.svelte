<script lang="ts">
  import { THEME_PRESETS } from '$lib/themes/presets'
  import { TOKEN_SCHEMA, validateToken } from '$lib/themes/token-schema'
  import { computeThemeTokens } from '$lib/themes/engine'
  import { dockApi } from '$lib/api/dock'
  import { messages } from '$lib/i18n'
  import type { DockPreferences, ThemeMode, SpacingPreset, RadiusPreset } from '$lib/types/dock'

  interface Props {
    preferences: DockPreferences
    onChange: (p: DockPreferences) => void
    onBack: () => void
    notify: (msg: string, kind?: 'success' | 'error') => void
  }

  let { preferences, onChange, onBack, notify }: Props = $props()

  let fonts = $state<string[]>([])
  let zhQuery = $state(preferences.fontFamilyZh)
  let enQuery = $state(preferences.fontFamilyEn)
  let zhOpen = $state(false)
  let enOpen = $state(false)
  let checkingUpdate = $state(false)
  let updateStatus = $state<'idle' | 'latest' | 'available' | 'error'>('idle')
  let updateVersion = $state('')
  let installing = $state(false)
  let proxyType = $state('http')
  let proxyHost = $state('')
  let proxyPort = $state('')
  let proxyError = $state('')
  let dataDir = $state('')
  let dataDirMode = $state<'default' | 'portable' | 'custom'>('default')
  let shortcutModifiers = $state(preferences.shortcutModifiers)
  let shortcutKey = $state(preferences.shortcutKey)
  let shortcutRegistered = $state(preferences.shortcutRegistered)
  let recordingShortcut = $state(false)
  let shortcutError = $state('')
  let quickModifiers = $state(preferences.quickAccessShortcutModifiers)
  let quickKey = $state(preferences.quickAccessShortcutKey)
  let quickRegistered = $state(preferences.quickAccessShortcutRegistered)
  let recordingQuick = $state(false)
  let quickError = $state('')
  let expertMode = $state(false)
  let overrides = $state<Record<string, string>>({ ...preferences.themeOverrides })
  let expertError = $state('')

  $effect(() => {
    dockApi.listFonts().then((f) => (fonts = f)).catch(() => {})
    dockApi.dataDirInfo().then((info) => {
      dataDir = info.path
      dataDirMode = info.mode
    }).catch(() => {})
    parseProxy(preferences.updateProxy)
    dockApi.shortcutStatus().then((s) => {
      shortcutModifiers = s.modifiers
      shortcutKey = s.key
      shortcutRegistered = s.registered
    }).catch(() => {})
    dockApi.quickAccessShortcutStatus().then((s) => {
      quickModifiers = s.modifiers
      quickKey = s.key
      quickRegistered = s.registered
    }).catch(() => {})
  })

  function parseProxy(p: string) {
    if (!p) return
    const match = p.match(/^(https?|socks5):\/\/([^:]+):(\d+)$/)
    if (match) {
      proxyType = match[1]
      proxyHost = match[2]
      proxyPort = match[3]
    }
  }

  function validateProxy(): string | null {
    if (!proxyHost && !proxyPort) return null
    if (!proxyHost) return messages.settings.proxyErrHostRequired
    if (!proxyPort) return messages.settings.proxyErrPortRequired
    if (!/^\d+$/.test(proxyPort)) return messages.settings.proxyErrPortNumeric
    if (+proxyPort < 1 || +proxyPort > 65535) return messages.settings.proxyErrPortRange
    if (proxyHost.includes('://')) return messages.settings.proxyErrNoProtocol
    if (proxyHost.includes(':')) return messages.settings.proxyErrNoPort
    return null
  }

  function saveProxy() {
    proxyError = ''
    const err = validateProxy()
    if (err) {
      proxyError = err
      return
    }
    const proxy = proxyHost ? `${proxyType}://${proxyHost}:${proxyPort}` : ''
    onChange({ ...preferences, updateProxy: proxy })
  }

  function clearProxy() {
    proxyHost = ''
    proxyPort = ''
    proxyError = ''
    onChange({ ...preferences, updateProxy: '' })
  }

  async function checkUpdate() {
    checkingUpdate = true
    updateStatus = 'idle'
    try {
      const { check } = await import('@tauri-apps/plugin-updater')
      const update = await check()
      if (update) {
        updateStatus = 'available'
        updateVersion = update.version
      } else {
        updateStatus = 'latest'
      }
    } catch {
      updateStatus = 'error'
    } finally {
      checkingUpdate = false
    }
  }

  async function installUpdate() {
    installing = true
    try {
      const { check } = await import('@tauri-apps/plugin-updater')
      const update = await check()
      if (update) {
        await update.downloadAndInstall()
        const { relaunch } = await import('@tauri-apps/plugin-process')
        await relaunch()
      }
    } catch (e) {
      notify(`${messages.settings.updateFailed}: ${e}`, 'error')
    } finally {
      installing = false
    }
  }

  async function pickDataDir() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({ directory: true, title: messages.settings.selectDataDirTitle })
      if (!selected) return
      const info = await dockApi.dataDirSet(selected as string)
      dataDir = info.path
      dataDirMode = info.mode
    } catch (e) {
      notify(`${messages.settings.changeDataDirFailed}: ${e}`, 'error')
    }
  }

  function selectPreset(id: string) {
    onChange({ ...preferences, themeMode: 'preset', themePresetId: id })
  }

  function setMode(mode: ThemeMode) {
    if (mode === 'custom' && !preferences.customBasePresetId) {
      onChange({ ...preferences, themeMode: mode, customBasePresetId: preferences.themePresetId })
    } else {
      onChange({ ...preferences, themeMode: mode })
    }
  }

  function applyOverride(key: string, value: string) {
    expertError = ''
    if (!value.trim()) {
      const next = { ...overrides }
      delete next[key]
      overrides = next
    } else {
      const err = validateToken(key, value.trim())
      if (err) {
        expertError = `${key}: ${err}`
        return
      }
      overrides = { ...overrides, [key]: value.trim() }
    }
    onChange({ ...preferences, themeOverrides: overrides })
  }

  function resetOverrides() {
    overrides = {}
    expertError = ''
    onChange({ ...preferences, themeOverrides: {} })
  }

  function startRecording(target: 'main' | 'quick') {
    shortcutError = ''
    quickError = ''
    if (target === 'main') recordingShortcut = true
    else recordingQuick = true
  }

  async function recordKey(e: KeyboardEvent, target: 'main' | 'quick') {
    e.preventDefault()
    e.stopPropagation()
    const mods: string[] = []
    if (e.ctrlKey) mods.push('Ctrl')
    if (e.altKey) mods.push('Alt')
    if (e.shiftKey) mods.push('Shift')
    if (e.metaKey) mods.push('Meta')
    const key = e.key
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(key)) return
    if (mods.length === 0) return
    const modStr = mods.join('+')
    const keyName = key.length === 1 ? key.toUpperCase() : key
    if (target === 'main') {
      recordingShortcut = false
      if (modStr === quickModifiers && keyName === quickKey) {
        shortcutError = messages.settings.shortcutConflict
        return
      }
      try {
        const s = await dockApi.shortcutUpdate(modStr, keyName)
        shortcutModifiers = s.modifiers
        shortcutKey = s.key
        shortcutRegistered = s.registered
        onChange({ ...preferences, shortcutModifiers: s.modifiers, shortcutKey: s.key, shortcutRegistered: s.registered })
      } catch (err) {
        shortcutError = String(err)
      }
    } else {
      recordingQuick = false
      if (modStr === shortcutModifiers && keyName === shortcutKey) {
        quickError = messages.settings.shortcutConflict
        return
      }
      try {
        const s = await dockApi.quickAccessShortcutUpdate(modStr, keyName)
        quickModifiers = s.modifiers
        quickKey = s.key
        quickRegistered = s.registered
        onChange({ ...preferences, quickAccessShortcutModifiers: s.modifiers, quickAccessShortcutKey: s.key, quickAccessShortcutRegistered: s.registered })
      } catch (err) {
        quickError = String(err)
      }
    }
  }

  let filteredZh = $derived(fonts.filter((f) => f.toLowerCase().includes(zhQuery.toLowerCase())))
  let filteredEn = $derived(fonts.filter((f) => f.toLowerCase().includes(enQuery.toLowerCase())))
  let themeMode = $derived(preferences.themeMode)

  const spacingPresets: [SpacingPreset, string][] = $derived([
    ['compact', messages.settingsNames?.compact ?? 'Compact'],
    ['normal', messages.settingsNames?.normal ?? 'Normal'],
    ['comfortable', messages.settingsNames?.comfortable ?? 'Comfortable'],
  ])
  const radiusPresets: [RadiusPreset, string][] = $derived([
    ['sharp', messages.settingsNames?.sharp ?? 'Sharp'],
    ['normal', messages.settingsNames?.normal ?? 'Normal'],
    ['round', messages.settingsNames?.round ?? 'Round'],
  ])
</script>

<div class="settings-view">
  <div class="settings-header">
    <button class="back-btn" onclick={onBack}>{messages.settings.back}</button>
  </div>

  <div class="settings-body">
    <!-- Data directory -->
    <section class="card">
      <span class="section-label">{messages.settings.dataDir}</span>
      <div class="row">
        <span class="row-label">{messages.settings.dataDirLabel}</span>
        <code class="path-text">{dataDir || messages.settings.loading}</code>
      </div>
      <div class="row">
        <button class="btn" onclick={pickDataDir}>{messages.settings.dataDirChange}</button>
      </div>
      {#if dataDirMode === 'custom'}
        <p class="section-subtitle">{messages.settings.dataDirRestartHint}</p>
      {/if}
    </section>

    <!-- Language -->
    <section class="card">
      <span class="section-label">{messages.settings.language} / LANGUAGE</span>
      <div class="theme-cards">
        <button class="theme-card" class:active={preferences.language === 'zh-CN'} onclick={() => onChange({ ...preferences, language: 'zh-CN' })}>
          <span class="card-name">中文</span>
        </button>
        <button class="theme-card" class:active={preferences.language === 'en'} onclick={() => onChange({ ...preferences, language: 'en' })}>
          <span class="card-name">English</span>
        </button>
      </div>
      <p class="lang-hint">{messages.settings.restartHint}</p>
    </section>

    <!-- Theme -->
    <section class="card">
      <span class="section-label">{messages.settings.theme}</span>
      <div class="seg">
        <button class="seg-btn" class:active={themeMode === 'preset'} onclick={() => setMode('preset')}>{messages.settings.theme}</button>
        <button class="seg-btn" class:active={themeMode === 'system'} onclick={() => setMode('system')}>{messages.settings.followSystem}</button>
        <button class="seg-btn" class:active={themeMode === 'custom'} onclick={() => setMode('custom')}>{messages.settings.advanced}</button>
      </div>
      {#if themeMode !== 'custom'}
        {#if themeMode === 'system'}
          <p class="section-subtitle">{messages.settings.followSystem}</p>
        {/if}
        <div class="theme-cards">
          {#each Object.values(THEME_PRESETS) as preset}
            <button
              class="theme-card"
              class:active={preferences.themePresetId === preset.id}
              onclick={() => selectPreset(preset.id)}
            >
              <div class="swatch" style="background:{preset.tokens['--surface-0']}">
                <span class="swatch-line accent" style="background:{preset.tokens['--color-primary']}"></span>
                <span class="swatch-line" style="background:{preset.tokens['--text-primary']}"></span>
                <span class="swatch-line short" style="background:{preset.tokens['--text-primary']}"></span>
              </div>
              <span class="card-name">{messages.themeNames[preset.id as keyof typeof messages.themeNames] || preset.name}</span>
            </button>
          {/each}
        </div>
      {:else}
        <div class="expert-panel">
          <div class="row">
            <label class="toggle-row">
              <input type="checkbox" bind:checked={expertMode} />
              <span>{messages.settings.expertMode}</span>
            </label>
            <button class="btn" onclick={resetOverrides}>{messages.settings.usePreset}</button>
          </div>
          <div class="theme-cards">
            {#each Object.values(THEME_PRESETS) as preset}
              <button
                class="theme-card"
                class:active={preferences.customBasePresetId === preset.id}
                onclick={() => onChange({ ...preferences, customBasePresetId: preset.id })}
              >
                <div class="swatch" style="background:{preset.tokens['--surface-0']}">
                  <span class="swatch-line accent" style="background:{preset.tokens['--color-primary']}"></span>
                  <span class="swatch-line" style="background:{preset.tokens['--text-primary']}"></span>
                  <span class="swatch-line short" style="background:{preset.tokens['--text-primary']}"></span>
                </div>
                <span class="card-name">{messages.themeNames[preset.id as keyof typeof messages.themeNames] || preset.name}</span>
              </button>
            {/each}
          </div>
          {#if expertError}
            <p class="expert-error">{expertError}</p>
          {/if}
          {#each TOKEN_SCHEMA as token}
            {@const label = messages.expert[token.key as keyof typeof messages.expert] ?? token.key}
            <div class="expert-row">
              <div class="expert-info">
                <span class="expert-key">{label}</span>
                <code class="expert-token">{token.key}</code>
              </div>
              {#if expertMode}
                <input
                  class="expert-input"
                  value={overrides[token.key] ?? ''}
                  placeholder={computeThemeTokens(preferences, true)[token.key] ?? ''}
                  onchange={(e) => applyOverride(token.key, e.currentTarget.value)}
                />
              {:else}
                <span class="expert-value">{overrides[token.key] ?? computeThemeTokens(preferences, true)[token.key] ?? '—'}</span>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </section>

    <!-- Font -->
    <section class="card">
      <span class="section-label">{messages.settings.font}</span>
      <div class="row">
        <span class="row-label">{messages.settings.zhFont}</span>
        <div class="combo">
          <input
            bind:value={zhQuery}
            placeholder={messages.settings.zhFontSearch}
            onfocus={() => (zhOpen = true)}
            onblur={() => setTimeout(() => (zhOpen = false), 150)}
            oninput={() => onChange({ ...preferences, fontFamilyZh: zhQuery })}
          />
          {#if zhOpen && filteredZh.length}
            <div class="dropdown">
              {#each filteredZh.slice(0, 8) as f}
                <button class="dropdown-item" onmousedown={() => { zhQuery = f; onChange({ ...preferences, fontFamilyZh: f }) }}>{f}</button>
              {/each}
            </div>
          {/if}
        </div>
      </div>
      <div class="row">
        <span class="row-label">{messages.settings.enFont}</span>
        <div class="combo">
          <input
            bind:value={enQuery}
            placeholder={messages.settings.enFontSearch}
            onfocus={() => (enOpen = true)}
            onblur={() => setTimeout(() => (enOpen = false), 150)}
            oninput={() => onChange({ ...preferences, fontFamilyEn: enQuery })}
          />
          {#if enOpen && filteredEn.length}
            <div class="dropdown">
              {#each filteredEn.slice(0, 8) as f}
                <button class="dropdown-item" onmousedown={() => { enQuery = f; onChange({ ...preferences, fontFamilyEn: f }) }}>{f}</button>
              {/each}
            </div>
          {/if}
        </div>
      </div>
      <div class="row">
        <span class="row-label">UI</span>
        <input type="range" min="10" max="16" step="0.5" value={preferences.uiTextSizePx} oninput={(e) => onChange({ ...preferences, uiTextSizePx: +e.currentTarget.value })} />
        <span class="range-val">{preferences.uiTextSizePx}px</span>
      </div>
      <div class="row">
        <span class="row-label">{messages.settings.font}</span>
        <input type="range" min="11" max="20" step="0.5" value={preferences.contentTextSizePx} oninput={(e) => onChange({ ...preferences, contentTextSizePx: +e.currentTarget.value })} />
        <span class="range-val">{preferences.contentTextSizePx}px</span>
      </div>
      <div class="row">
        <span class="row-label">Spacing</span>
        <div class="seg">
          {#each spacingPresets as [value, label]}
            <button class="seg-btn" class:active={preferences.spacingPreset === value} onclick={() => onChange({ ...preferences, spacingPreset: value })}>{label}</button>
          {/each}
        </div>
      </div>
      <div class="row">
        <span class="row-label">Radius</span>
        <div class="seg">
          {#each radiusPresets as [value, label]}
            <button class="seg-btn" class:active={preferences.radiusPreset === value} onclick={() => onChange({ ...preferences, radiusPreset: value })}>{label}</button>
          {/each}
        </div>
      </div>
    </section>

    <!-- Update -->
    <section class="card">
      <span class="section-label">{messages.settings.update}</span>
      <div class="row">
        <span class="row-label">{messages.settings.autoCleanupDays}</span>
        <input type="number" min="0" max="30" value={preferences.autoCleanupDays} onchange={(e) => onChange({ ...preferences, autoCleanupDays: +e.currentTarget.value })} class="num-input" />
        <span class="range-val">{preferences.autoCleanupDays === 0 ? messages.settings.autoCleanupHint : `${preferences.autoCleanupDays}`}</span>
      </div>
      <p class="section-subtitle">{messages.settings.proxyNote}</p>
      <div class="row">
        <span class="row-label">{messages.settings.proxyType}</span>
        <select bind:value={proxyType}>
          <option value="http">HTTP</option>
          <option value="https">HTTPS</option>
          <option value="socks5">SOCKS5</option>
        </select>
      </div>
      <div class="row">
        <span class="row-label">{messages.settings.proxyHost}</span>
        <input bind:value={proxyHost} placeholder={messages.settings.proxyHostExample} />
      </div>
      <div class="row">
        <span class="row-label">{messages.settings.proxyPort}</span>
        <input bind:value={proxyPort} placeholder={messages.settings.proxyPortExample} />
      </div>
      {#if proxyError}
        <p class="proxy-error">{proxyError}</p>
      {/if}
      <div class="row">
        <button class="btn" onclick={saveProxy}>{messages.settings.saveProxy}</button>
        <button class="btn" onclick={clearProxy}>{messages.settings.clear}</button>
      </div>
      <div class="row">
        {#if updateStatus === 'available'}
          <button class="btn accent" onclick={installUpdate} disabled={installing}>
            {installing ? messages.settings.downloading : `${messages.settings.updateNow} v${updateVersion}`}
          </button>
        {:else}
          <button class="btn" onclick={checkUpdate} disabled={checkingUpdate}>
            {checkingUpdate ? messages.settings.checking : messages.settings.checkUpdate}
          </button>
        {/if}
        {#if updateStatus === 'latest'}
          <span class="update-status">{messages.settings.upToDate}</span>
        {:else if updateStatus === 'error'}
          <span class="update-status error">{messages.settings.checkFailed}</span>
        {/if}
      </div>
    </section>

    <!-- System -->
    <section class="card">
      <span class="section-label">{messages.settings.system}</span>
      <label class="toggle-row">
        <input type="checkbox" checked={preferences.launchOnStartup} onchange={(e) => onChange({ ...preferences, launchOnStartup: e.currentTarget.checked })} />
        <span>{messages.settings.launchOnStartup}</span>
      </label>
    </section>

    <!-- Shortcut -->
    <section class="card">
      <span class="section-label">{messages.settings.shortcut}</span>
      <div class="row">
        <span class="row-label">{messages.settings.shortcutMainLabel}</span>
        <code class="shortcut-display">{shortcutModifiers}+{shortcutKey}</code>
        <span class="shortcut-status" class:ok={shortcutRegistered}>
          {shortcutRegistered ? messages.settings.shortcutRegistered : messages.settings.shortcutFailed}
        </span>
      </div>
      {#if recordingShortcut}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="shortcut-recorder" tabindex="-1" onkeydown={(e) => recordKey(e, 'main')}>
          {messages.settings.shortcutRecording}
        </div>
        <p class="section-subtitle">{messages.settings.shortcutHint}</p>
      {:else}
        <div class="row">
          <button class="btn" onclick={() => startRecording('main')}>{messages.settings.shortcutRecord}</button>
        </div>
      {/if}
      {#if shortcutError}
        <p class="proxy-error">{shortcutError}</p>
      {/if}
      <div class="row">
        <span class="row-label">{messages.settings.shortcutQuickLabel}</span>
        <code class="shortcut-display">{quickModifiers}+{quickKey}</code>
        <span class="shortcut-status" class:ok={quickRegistered}>
          {quickRegistered ? messages.settings.shortcutRegistered : messages.settings.shortcutFailed}
        </span>
      </div>
      {#if recordingQuick}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="shortcut-recorder" tabindex="-1" onkeydown={(e) => recordKey(e, 'quick')}>
          {messages.settings.shortcutRecording}
        </div>
        <p class="section-subtitle">{messages.settings.shortcutHint}</p>
      {:else}
        <div class="row">
          <button class="btn" onclick={() => startRecording('quick')}>{messages.settings.shortcutRecord}</button>
        </div>
      {/if}
      {#if quickError}
        <p class="proxy-error">{quickError}</p>
      {/if}
    </section>
  </div>
</div>

<style>
  .settings-view {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .settings-header {
    padding: 0.3rem 0.5rem;
    border-bottom: 1px solid var(--border-subtle);
  }

  .back-btn {
    background: none;
    border: 0;
    color: var(--text-muted);
    font-size: max(var(--font-sm, 0.72rem), 0.72rem);
    cursor: pointer;
    padding: 0.3rem 0.4rem;
    border-radius: var(--radius-sm);
  }

  .back-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
  }

  .settings-body {
    flex: 1;
    overflow-y: auto;
    padding: 0.45rem;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }

  .card {
    background: var(--surface-1);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    padding: 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }

  .section-label {
    font-size: max(var(--font-xs, 0.65rem), 0.65rem);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .section-subtitle {
    font-size: max(var(--font-xs, 0.62rem), 0.62rem);
    color: var(--text-faint);
    margin: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .row-label {
    font-size: max(var(--font-sm, 0.68rem), 0.68rem);
    color: var(--text-muted);
    flex-shrink: 0;
    min-width: 4.5rem;
  }

  .btn {
    padding: 0.3rem 0.7rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    font-size: max(var(--font-sm, 0.68rem), 0.68rem);
    cursor: pointer;
    transition: background 0.12s, border-color 0.12s;
  }

  .btn:hover {
    background: color-mix(in srgb, var(--text-primary) 8%, var(--surface-2));
    border-color: var(--border-emphasis);
  }

  .btn.accent {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border-color: color-mix(in srgb, var(--color-primary) 35%, transparent);
    color: var(--color-primary);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .seg {
    display: flex;
    gap: 0.2rem;
    background: var(--surface-2);
    border-radius: var(--radius-md);
    padding: 0.15rem;
  }

  .seg-btn {
    flex: 1;
    padding: 0.3rem 0.4rem;
    background: none;
    border: 0;
    border-radius: calc(var(--radius-md, 0.35rem) - 0.1rem);
    color: var(--text-muted);
    font-size: max(var(--font-sm, 0.68rem), 0.68rem);
    cursor: pointer;
    transition: background 0.12s, color 0.12s;
  }

  .seg-btn.active {
    background: color-mix(in srgb, var(--color-primary) 14%, transparent);
    color: var(--color-primary);
  }

  .theme-cards {
    display: flex;
    gap: 0.4rem;
  }

  .theme-card {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.25rem;
    padding: 0.35rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    cursor: pointer;
    transition: border-color 0.12s;
  }

  .theme-card.active {
    border-color: color-mix(in srgb, var(--color-primary) 50%, transparent);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--color-primary) 30%, transparent);
  }

  .swatch {
    width: 100%;
    height: 1.9rem;
    border-radius: 0.2rem;
    border: 1px solid var(--border-default);
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 0.18rem;
    padding: 0 0.35rem;
    box-sizing: border-box;
    overflow: hidden;
  }

  .swatch-line {
    display: block;
    height: 0.22rem;
    width: 78%;
    border-radius: 999px;
    opacity: 0.75;
  }

  .swatch-line.accent {
    width: 45%;
    opacity: 1;
  }

  .swatch-line.short {
    width: 58%;
    opacity: 0.45;
  }

  .card-name {
    font-size: max(var(--font-xs, 0.62rem), 0.62rem);
    color: var(--text-primary);
    white-space: nowrap;
  }

  .expert-panel {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .expert-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.2rem 0;
    border-bottom: 1px solid var(--border-subtle);
  }

  .expert-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .expert-key {
    font-size: max(var(--font-sm, 0.68rem), 0.68rem);
    color: var(--text-primary);
  }

  .expert-token {
    font-size: max(var(--font-xs, 0.62rem), 0.62rem);
    color: var(--text-faint);
  }

  .expert-input {
    width: 9rem;
    padding: 0.25rem 0.4rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font-size: max(var(--font-xs, 0.65rem), 0.65rem);
    font-family: inherit;
    outline: none;
  }

  .expert-input:focus {
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
  }

  .expert-value {
    font-size: max(var(--font-xs, 0.62rem), 0.62rem);
    color: var(--text-muted);
    max-width: 9rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .expert-error {
    font-size: max(var(--font-xs, 0.62rem), 0.62rem);
    color: var(--color-danger);
    margin: 0;
  }

  .combo {
    position: relative;
    flex: 1;
  }

  .combo input {
    width: 100%;
    padding: 0.3rem 0.45rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    font-size: max(var(--font-sm, 0.68rem), 0.68rem);
    font-family: inherit;
    outline: none;
    box-sizing: border-box;
  }

  .combo input:focus {
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
  }

  .dropdown {
    position: absolute;
    top: calc(100% + 2px);
    left: 0;
    right: 0;
    max-height: 10rem;
    overflow-y: auto;
    background: var(--surface-0);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-default);
    z-index: 10;
  }

  .dropdown-item {
    display: block;
    width: 100%;
    padding: 0.3rem 0.45rem;
    background: none;
    border: 0;
    color: var(--text-primary);
    font-size: max(var(--font-sm, 0.68rem), 0.68rem);
    text-align: left;
    cursor: pointer;
  }

  .dropdown-item:hover {
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
  }

  input[type='range'] {
    flex: 1;
    accent-color: var(--color-primary);
  }

  .range-val {
    font-size: max(var(--font-xs, 0.65rem), 0.65rem);
    color: var(--text-muted);
    flex-shrink: 0;
    min-width: 2.5rem;
    text-align: right;
  }

  .num-input {
    width: 3.5rem;
    padding: 0.25rem 0.35rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    font-size: max(var(--font-sm, 0.68rem), 0.68rem);
    font-family: inherit;
    outline: none;
  }

  select {
    padding: 0.3rem 0.45rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    font-size: max(var(--font-sm, 0.68rem), 0.68rem);
    font-family: inherit;
    outline: none;
  }

  .row input:not([type='range']):not([type='checkbox']):not(.num-input):not(.expert-input) {
    flex: 1;
    padding: 0.3rem 0.45rem;
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    font-size: max(var(--font-sm, 0.68rem), 0.68rem);
    font-family: inherit;
    outline: none;
  }

  .proxy-error {
    font-size: max(var(--font-xs, 0.62rem), 0.62rem);
    color: var(--color-danger);
    margin: 0;
  }

  .update-status {
    font-size: max(var(--font-sm, 0.68rem), 0.68rem);
    color: var(--color-success);
  }

  .update-status.error {
    color: var(--color-danger);
  }

  .toggle-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: max(var(--font-sm, 0.72rem), 0.72rem);
    color: var(--text-primary);
    cursor: pointer;
  }

  .toggle-row input[type='checkbox'] {
    accent-color: var(--color-primary);
  }

  .path-text {
    flex: 1;
    font-size: max(var(--font-xs, 0.62rem), 0.62rem);
    color: var(--text-muted);
    word-break: break-all;
    user-select: text;
  }

  .shortcut-display {
    font-size: max(var(--font-sm, 0.68rem), 0.68rem);
    color: var(--text-primary);
    background: var(--surface-2);
    padding: 0.2rem 0.45rem;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-default);
  }

  .shortcut-status {
    font-size: max(var(--font-xs, 0.62rem), 0.62rem);
    color: var(--color-danger);
  }

  .shortcut-status.ok {
    color: var(--color-success);
  }

  .shortcut-recorder {
    padding: 0.5rem;
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    border: 1px dashed color-mix(in srgb, var(--color-primary) 40%, transparent);
    border-radius: var(--radius-md);
    color: var(--color-primary);
    font-size: max(var(--font-sm, 0.72rem), 0.72rem);
    text-align: center;
    outline: none;
  }

  .lang-hint {
    font-size: max(var(--font-xs, 0.62rem), 0.62rem);
    color: var(--text-faint);
    margin: 0;
  }
</style>
