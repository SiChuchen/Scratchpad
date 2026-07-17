<script lang="ts">
  // QuickAccessApp —— 全局资料库快速入口窗口的根组件。
  //
  // 行为：
  //   * onMount 加载 DockPreferences，应用主题 tokens 和 locale；
  //   * 监听 `quick-access-focus-input` 事件，每次重新聚焦当前模式输入；
  //   * 监听 `vault-sensitive-reset` 事件，重新掩码已显示的敏感值；
  //   * 全局 keydown：Ctrl+Tab 切换 record/search；Escape 隐藏窗口。
  //
  // 两个模式始终挂载，仅隐藏非活动面板，因此切换模式或隐藏窗口不会丢失工作。

  import { onMount, onDestroy } from 'svelte'
  import { listen, type UnlistenFn } from '@tauri-apps/api/event'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { invoke } from '@tauri-apps/api/core'

  import { computeThemeTokens } from '$lib/themes/engine'
  import { loadLocale, detectLanguage, messages } from '$lib/i18n'
  import type { DockPreferences } from '$lib/types/dock'
  import type { QuickAccessState } from '$lib/types/quick-access'
  import { handleKeydown } from '$lib/state/quick-access'
  import { listenForPreferenceChanges } from '$lib/state/preferences-sync'
  import CaptureMode from '$lib/components/quick-access/CaptureMode.svelte'
  import SearchMode from '$lib/components/quick-access/SearchMode.svelte'
  import type { VaultEntryDetail } from '$lib/types/vault'

  const win = getCurrentWindow()

  // Element refs
  let unlisteners: UnlistenFn[] = []

  let mode = $state<'record' | 'search'>('record')
  let preferences = $state<DockPreferences | null>(null)
  let systemDark = $state(true)
  // Bumps each time the window blurs / sensitive-reset fires; SearchMode
  // forwards this to CopyableValue to re-mask sensitive values.
  let sensitiveResetToken = $state(0)
  let autoHybridSearch = $state(false)
  let aiConfigured = $state(false)
  let autoEnrich = $state(true)

  // Inline toast notification (simple ephemeral banner).
  let noticeText = $state('')
  let noticeKind = $state<'success' | 'error'>('success')
  let noticeTimer: ReturnType<typeof setTimeout> | null = null

  function notify(
    text: string,
    kind: 'success' | 'error' = 'success',
    _undo?: () => void,
    _actionLabel?: string,
  ) {
    noticeText = text
    noticeKind = kind
    if (noticeTimer) clearTimeout(noticeTimer)
    noticeTimer = setTimeout(() => {
      noticeText = ''
    }, kind === 'error' ? 6000 : 3000)
  }

  function onCaptureSaved(_entry: VaultEntryDetail) {
    // Future Task 17 / 18 could route this to a recent-entries list.
    // For now we just rely on the success notification.
  }

  function applyPreferences(next: DockPreferences) {
    if (next.language && preferences?.language !== next.language) {
      loadLocale(next.language)
    }
    preferences = next
  }

  async function onOpenAiSettings() {
    try {
      await invoke('ipc_open_main_settings')
    } catch {
      notify(messages.quickAccess.openSettingsFailed, 'error')
    }
  }

  /** 重新读取 AI 配置 + 设置；quick-access 每次聚焦时都要调，避免使用 stale 快照。 */
  async function reloadAiState() {
    try {
      const { vaultApi } = await import('$lib/api/vault')
      const [cfg, settings] = await Promise.all([
        vaultApi.getLlmConfig(),
        vaultApi.getAiSettings(),
      ])
      aiConfigured = !!cfg?.hasApiKey
      autoEnrich = settings.autoEnrich
      autoHybridSearch = settings.autoHybridSearch
    } catch (e) {
      console.error('QuickAccessApp: failed to reload AI state', e)
    }
  }

  onMount(async () => {
    // Sync initial system dark mode
    if (typeof window !== 'undefined' && window.matchMedia) {
      systemDark = window.matchMedia('(prefers-color-scheme: dark)').matches
    }

    try {
      const prefs = await invoke<DockPreferences>('ipc_preferences_get')
      if (!prefs.language) {
        prefs.language = detectLanguage()
      }
      applyPreferences(prefs)
    } catch (e) {
      console.error('QuickAccessApp: failed to load preferences', e)
    }

    // 初始加载 AI 配置（在 quick-access 第一次挂载时）。
    await reloadAiState()

    // Reactive system dark mode
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const onSystemTheme = (e: MediaQueryListEvent) => {
      systemDark = e.matches
    }
    mq.addEventListener('change', onSystemTheme)
    unlisteners.push(() => mq.removeEventListener('change', onSystemTheme))

    // Focus input when window is shown
    unlisteners.push(
      await listen('quick-access-focus-input', () => {
        focusActiveModeInput()
        // Reload preferences so theme/locale changes in main window reflect here.
        invoke<DockPreferences>('ipc_preferences_get')
          .then((prefs) => {
            applyPreferences(prefs)
          })
          .catch(() => {})
        // 关键：每次呼出都重读 AI 配置 — 用户可能在主窗口刚保存配置，
        // 否则 quick-access 持续使用首次挂载时的 stale 快照（"成功但没整理"）。
        void reloadAiState()
      }),
    )

    unlisteners.push(
      await listenForPreferenceChanges((next) => {
        applyPreferences(next)
      }),
    )

    // Sensitive reset on blur (window will also hide from Rust side)
    unlisteners.push(
      await listen('vault-sensitive-reset', () => {
        sensitiveResetToken += 1
      }),
    )
  })

  onDestroy(() => {
    for (const un of unlisteners) {
      try {
        un()
      } catch {}
    }
    unlisteners = []
  })

  // Apply theme tokens as CSS variables.
  $effect(() => {
    if (!preferences) return
    const tokens = computeThemeTokens(preferences, systemDark)
    const root = document.documentElement.style
    for (const [key, value] of Object.entries(tokens)) {
      root.setProperty(key, value)
    }
    root.setProperty('--font-family-zh', preferences.fontFamilyZh)
    root.setProperty('--font-family-en', preferences.fontFamilyEn)
  })

  function focusActiveModeInput() {
    queueMicrotask(() => {
      if (mode === 'record') {
        // CaptureMode owns its textarea; focus via DOM query.
        const ta = document.querySelector<HTMLTextAreaElement>(
          '.capture-mode .raw-textarea',
        )
        ta?.focus()
      } else {
        // SearchMode owns its input; focus via DOM query.
        const input = document.querySelector<HTMLInputElement>(
          '.mode-search .search-input',
        )
        input?.focus()
      }
    })
  }

  function requestHide() {
    win.hide().catch((e: unknown) => console.error('hide failed:', e))
  }

  function switchMode(next: 'record' | 'search') {
    mode = next
    focusActiveModeInput()
  }

  function onKeydown(e: KeyboardEvent) {
    // Build a transient QuickAccessState view for the pure helper.
    const snapshot: QuickAccessState = {
      mode,
      draft: '',
      query: '',
      selectedId: null,
    }
    handleKeydown(snapshot, e, requestHide)
    if (snapshot.mode !== mode) {
      switchMode(snapshot.mode)
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<svelte:window onkeydown={onKeydown} />

<main class="quick-shell">
  {#if noticeText}
    <div class="notice" class:error={noticeKind === 'error'} role="status" aria-live="polite">{noticeText}</div>
  {/if}

  <div class="qa-tablist" role="tablist" aria-label={messages.quickAccess.modeLabel}>
    <button
      id="qa-record-tab"
      type="button"
      role="tab"
      aria-selected={mode === 'record'}
      aria-controls="qa-record-panel"
      tabindex={mode === 'record' ? 0 : -1}
      class="qa-tab"
      class:active={mode === 'record'}
      onclick={() => switchMode('record')}
    >
      {messages.quickAccess.record}
    </button>
    <button
      id="qa-search-tab"
      type="button"
      role="tab"
      aria-selected={mode === 'search'}
      aria-controls="qa-search-panel"
      tabindex={mode === 'search' ? 0 : -1}
      class="qa-tab"
      class:active={mode === 'search'}
      onclick={() => switchMode('search')}
    >
      {messages.quickAccess.search}
    </button>
  </div>

  <div
    id="qa-record-panel"
    class="qa-panel"
    role="tabpanel"
    aria-labelledby="qa-record-tab"
    hidden={mode !== 'record'}
  >
    <CaptureMode
      {notify}
      {aiConfigured}
      {autoEnrich}
      onSaved={onCaptureSaved}
      onOpenSettings={onOpenAiSettings}
    />
  </div>
  <div
    id="qa-search-panel"
    class="qa-panel"
    role="tabpanel"
    aria-labelledby="qa-search-tab"
    hidden={mode !== 'search'}
  >
    <SearchMode
      {notify}
      resetToken={sensitiveResetToken}
      autoHybridSearch={autoHybridSearch}
    />
  </div>
</main>

<style>
  .quick-shell {
    width: 100vw;
    height: 100vh;
    min-width: 320px;
    min-height: 240px;
    display: flex;
    flex-direction: column;
    background: var(--surface-0);
    backdrop-filter: blur(24px);
    border: 1px solid var(--border-emphasis);
    box-shadow: var(--shadow-default);
    overflow: hidden;
  }

  .qa-tablist {
    display: flex;
    gap: 0;
    padding: 0.4rem 0.4rem 0;
    border-bottom: 1px solid var(--border-emphasis, rgba(255, 255, 255, 0.06));
    background: color-mix(in srgb, var(--surface-0) 80%, transparent);
  }
  .qa-tab {
    appearance: none;
    border: none;
    background: transparent;
    color: var(--text-muted, rgba(255, 255, 255, 0.55));
    padding: 0.4rem 0.9rem;
    font-size: var(--font-md, 14px);
    font-weight: 500;
    cursor: pointer;
    border-radius: 6px 6px 0 0;
    border-bottom: 2px solid transparent;
    transition: color 0.12s, border-color 0.12s, background 0.12s;
  }
  .qa-tab:hover {
    color: var(--text-strong, rgba(255, 255, 255, 0.9));
    background: color-mix(in srgb, var(--color-primary, #4f46e5) 8%, transparent);
  }
  .qa-tab.active {
    color: var(--color-primary, #4f46e5);
    border-bottom-color: var(--color-primary, #4f46e5);
  }

  .qa-panel {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .qa-panel[hidden] {
    display: none;
  }

  .notice {
    padding: 0.4rem 0.65rem;
    margin: 0.5rem 0.5rem 0;
    border-radius: var(--radius-md, 6px);
    font-size: var(--font-sm, 13px);
    background: color-mix(in srgb, var(--color-primary, #4f46e5) 12%, transparent);
    color: var(--color-primary, #4f46e5);
    border: 1px solid color-mix(in srgb, var(--color-primary, #4f46e5) 30%, transparent);
  }

  .notice.error {
    background: color-mix(in srgb, var(--color-danger, #ef4444) 12%, transparent);
    color: var(--color-danger, #ef4444);
    border-color: color-mix(in srgb, var(--color-danger, #ef4444) 30%, transparent);
  }
</style>
