<script lang="ts">
  import { onMount } from 'svelte'
  import type { DockPreferences } from '$lib/types/dock'
  import { dockApi } from '$lib/api/dock'

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
  let proxyDraft = $state('')

  let appVersion = $state('')
  let updateStatus = $state<'idle' | 'checking' | 'up-to-date' | 'available' | 'downloading' | 'error'>('idle')
  let updateErrorMsg = $state('')
  let pendingUpdate: any = $state(null)

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
    proxyDraft = preferences.updateProxy
  })

  function update(partial: Partial<DockPreferences>) {
    onChange({ ...preferences, ...partial })
  }

  function getProxyUrl(): string | undefined {
    const p = preferences.updateProxy.trim()
    if (!p) return undefined
    return p.startsWith('http') ? p : `http://${p}`
  }

  function saveProxy() {
    update({ updateProxy: proxyDraft.trim() })
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

  async function handleReset() {
    const defaultZh = installedFonts.find(f => f === 'Microsoft YaHei') || 'Microsoft YaHei'
    const defaultEn = installedFonts.find(f => f === 'Segoe UI') || 'Segoe UI'
    onChange({
      entrySurfaceOpacity: 0.82,
      dockBgOpacity: 0.85,
      dockBgColor: '#2a3548',
      dockMinimized: false,
      dockPositionX: preferences.dockPositionX,
      dockPositionY: preferences.dockPositionY,
      dockWidth: preferences.dockWidth,
      dockHeight: preferences.dockHeight,
      dockEdgeAnchor: 'right',
      textSizePx: 15,
      textColor: '#e8edf5',
      fontFamilyZh: defaultZh,
      fontFamilyEn: defaultEn,
      launchOnStartup: false,
      updateProxy: '',
    })
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
</script>

<div class="settings-view">
  <div class="settings-header">
    <button class="back-btn" onclick={onBack}>← 返回</button>
    <span class="settings-title">设置</span>
  </div>

  <div class="settings-body">
    <label class="setting-row">
      <span class="setting-label">条目透明度</span>
      <input
        type="range" min="0.35" max="1" step="0.01"
        value={preferences.entrySurfaceOpacity}
        oninput={(e) => update({ entrySurfaceOpacity: parseFloat((e.target as HTMLInputElement).value) })}
      />
      <span class="setting-value">{(preferences.entrySurfaceOpacity * 100).toFixed(0)}%</span>
    </label>

    <label class="setting-row">
      <span class="setting-label">背景透明度</span>
      <input
        type="range" min="0.3" max="1" step="0.01"
        value={preferences.dockBgOpacity}
        oninput={(e) => update({ dockBgOpacity: parseFloat((e.target as HTMLInputElement).value) })}
      />
      <span class="setting-value">{(preferences.dockBgOpacity * 100).toFixed(0)}%</span>
    </label>

    <label class="setting-row">
      <span class="setting-label">背景颜色</span>
      <input
        type="color"
        value={preferences.dockBgColor}
        oninput={(e) => update({ dockBgColor: (e.target as HTMLInputElement).value })}
      />
    </label>

    <label class="setting-row">
      <span class="setting-label">字号</span>
      <input
        type="range" min="12" max="22" step="1"
        value={preferences.textSizePx}
        oninput={(e) => update({ textSizePx: parseFloat((e.target as HTMLInputElement).value) })}
      />
      <span class="setting-value">{preferences.textSizePx}px</span>
    </label>

    <label class="setting-row">
      <span class="setting-label">文字颜色</span>
      <input
        type="color"
        value={preferences.textColor}
        oninput={(e) => update({ textColor: (e.target as HTMLInputElement).value })}
      />
    </label>

    <div class="setting-row">
      <span class="setting-label">中文字体</span>
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

    <div class="setting-row">
      <span class="setting-label">英文字体</span>
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

    <label class="setting-row">
      <span class="setting-label">开机自启动</span>
      <input
        type="checkbox"
        checked={preferences.launchOnStartup}
        onchange={() => update({ launchOnStartup: !preferences.launchOnStartup })}
      />
    </label>

    <div class="settings-divider"></div>

    <div class="setting-row">
      <span class="setting-label">更新代理</span>
      <div class="proxy-field">
        <input
          type="text"
          class="proxy-input"
          placeholder="例如 127.0.0.1:7890"
          bind:value={proxyDraft}
        />
        <button class="proxy-save-btn" disabled={proxyDraft === preferences.updateProxy} onclick={saveProxy}>保存</button>
      </div>
    </div>

    <div class="settings-divider"></div>

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

    <div class="setting-row" style="margin-top: 0.5rem;">
      <span class="setting-label"></span>
      <button class="reset-btn" onclick={handleReset}>重置为默认</button>
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
    border-bottom: 1px solid rgba(148, 163, 184, 0.08);
  }

  .back-btn {
    background: none;
    border: 1px solid transparent;
    color: color-mix(in srgb, var(--dock-text-color) 60%, transparent);
    font-size: 0.65rem;
    cursor: pointer;
    padding: 0.2rem 0.35rem;
    border-radius: 0.25rem;
    font-family: inherit;
    transition: color 0.12s, background 0.12s;
  }

  .back-btn:hover {
    color: var(--dock-text-color);
    background: color-mix(in srgb, var(--dock-text-color) 10%, transparent);
  }

  .settings-title {
    font-size: 0.7rem;
    font-weight: 700;
    color: rgba(148, 163, 184, 0.85);
  }

  .settings-body {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding-top: 0.5rem;
  }

  .setting-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.65rem;
    color: rgba(225, 238, 247, 0.75);
  }

  .setting-label {
    min-width: 5rem;
    color: rgba(148, 163, 184, 0.7);
  }

  .setting-value {
    font-size: 0.55rem;
    color: rgba(148, 163, 184, 0.5);
    min-width: 3rem;
    text-align: right;
  }

  input[type="range"] {
    flex: 1;
    accent-color: rgba(125, 211, 252, 0.7);
  }

  input[type="color"] {
    width: 1.5rem;
    height: 1.5rem;
    border: 1px solid rgba(148, 163, 184, 0.15);
    border-radius: 0.25rem;
    background: none;
    cursor: pointer;
    padding: 0;
  }

  input[type="checkbox"] {
    accent-color: rgba(125, 211, 252, 0.7);
  }

  .font-picker {
    flex: 1;
    position: relative;
  }

  .font-input {
    width: 100%;
    background: rgba(15, 23, 42, 0.5);
    border: 1px solid rgba(148, 163, 184, 0.15);
    border-radius: 0.25rem;
    color: #e5eef7;
    font-size: 0.6rem;
    padding: 0.2rem 0.3rem;
    font-family: inherit;
  }

  .font-input:focus {
    outline: none;
    border-color: rgba(125, 211, 252, 0.4);
  }

  .font-input::placeholder {
    color: rgba(148, 163, 184, 0.35);
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
    background: rgba(15, 23, 42, 0.95);
    border: 1px solid rgba(148, 163, 184, 0.15);
    border-top: none;
    border-radius: 0 0 0.25rem 0.25rem;
    z-index: 50;
  }

  .font-item {
    padding: 0.2rem 0.4rem;
    font-size: 0.55rem;
    color: rgba(225, 238, 247, 0.75);
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .font-item:hover {
    background: rgba(125, 211, 252, 0.1);
    color: rgba(125, 211, 252, 0.9);
  }

  .font-item.active {
    background: rgba(125, 211, 252, 0.15);
    color: rgba(125, 211, 252, 0.95);
  }

  .reset-btn {
    background: rgba(15, 23, 42, 0.5);
    border: 1px solid rgba(148, 163, 184, 0.2);
    color: rgba(248, 113, 113, 0.85);
    font-size: 0.6rem;
    padding: 0.25rem 0.75rem;
    border-radius: 0.3rem;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s, border-color 0.12s;
  }

  .reset-btn:hover {
    background: rgba(248, 113, 113, 0.1);
    border-color: rgba(248, 113, 113, 0.3);
  }

  .settings-divider {
    height: 1px;
    background: rgba(148, 163, 184, 0.08);
    margin: 0.25rem 0;
  }

  .proxy-field {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  .proxy-input {
    flex: 1;
    background: rgba(15, 23, 42, 0.5);
    border: 1px solid rgba(148, 163, 184, 0.15);
    border-radius: 0.25rem;
    color: #e5eef7;
    font-size: 0.6rem;
    padding: 0.2rem 0.3rem;
    font-family: inherit;
    min-width: 0;
  }

  .proxy-input:focus {
    outline: none;
    border-color: rgba(125, 211, 252, 0.4);
  }

  .proxy-input::placeholder {
    color: rgba(148, 163, 184, 0.35);
  }

  .proxy-save-btn {
    background: rgba(15, 23, 42, 0.5);
    border: 1px solid rgba(148, 163, 184, 0.2);
    color: rgba(148, 163, 184, 0.7);
    font-size: 0.6rem;
    padding: 0.2rem 0.5rem;
    border-radius: 0.25rem;
    cursor: pointer;
    font-family: inherit;
    flex-shrink: 0;
    transition: background 0.12s, color 0.12s;
  }

  .proxy-save-btn:hover:not(:disabled) {
    background: rgba(148, 163, 184, 0.1);
    color: rgba(225, 238, 247, 0.9);
  }

  .proxy-save-btn:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .about-section {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.35rem 0;
  }

  .about-header {
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
  }

  .about-title {
    font-size: 0.7rem;
    font-weight: 600;
    color: rgba(225, 238, 247, 0.85);
  }

  .about-version {
    font-size: 0.6rem;
    color: rgba(148, 163, 184, 0.5);
  }

  .update-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .update-status {
    font-size: 0.6rem;
    color: rgba(148, 163, 184, 0.5);
  }

  .update-status.ok {
    color: rgba(74, 222, 128, 0.8);
  }

  .update-status.available {
    color: rgba(125, 211, 252, 0.9);
    font-weight: 500;
  }

  .update-status.err {
    color: rgba(248, 113, 113, 0.85);
  }

  .update-btn {
    background: rgba(15, 23, 42, 0.5);
    border: 1px solid rgba(148, 163, 184, 0.2);
    color: rgba(148, 163, 184, 0.7);
    font-size: 0.6rem;
    padding: 0.2rem 0.6rem;
    border-radius: 0.25rem;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s, color 0.12s, border-color 0.12s;
  }

  .update-btn:hover {
    background: rgba(148, 163, 184, 0.1);
    color: rgba(225, 238, 247, 0.9);
    border-color: rgba(148, 163, 184, 0.3);
  }

  .install-btn {
    background: rgba(125, 211, 252, 0.12);
    border-color: rgba(125, 211, 252, 0.3);
    color: rgba(125, 211, 252, 0.9);
    font-weight: 500;
  }

  .install-btn:hover {
    background: rgba(125, 211, 252, 0.22);
    border-color: rgba(125, 211, 252, 0.5);
  }
</style>
