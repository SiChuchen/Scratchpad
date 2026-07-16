<script lang="ts">
  // QuickAccessApp —— 全局资料库快速入口窗口的根组件。
  //
  // 行为：
  //   * onMount 加载 DockPreferences，应用主题 tokens 和 locale；
  //   * 监听 `quick-access-focus-input` 事件，每次重新聚焦当前模式输入；
  //   * 监听 `vault-sensitive-reset` 事件，清空当前 mode 的草稿/查询；
  //   * 全局 keydown：Ctrl+Tab 切换 record/search；Escape 隐藏窗口。
  //
  // 窗口隐藏不销毁 WebView，因此未保存的 draft/query/selectedId 自然保留。
  //
  // 实现说明：使用 primitive/union 类型的 $state 字段（而非 $state<ObjectType>）。
  // svelte-check 4.4.6 + Svelte 5.55 在多个 $state<复杂对象>(...) 声明并存时
  // 存在检测异常，把每个字段拆成单独的 primitive $state 即可规避。

  import { onMount, onDestroy } from 'svelte'
  import { listen, type UnlistenFn } from '@tauri-apps/api/event'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { invoke } from '@tauri-apps/api/core'

  import { computeThemeTokens } from '$lib/themes/engine'
  import { loadLocale, detectLanguage } from '$lib/i18n'
  import type { DockPreferences } from '$lib/types/dock'
  import type { QuickAccessState } from '$lib/types/quick-access'
  import { handleKeydown } from '$lib/state/quick-access'
  import CaptureMode from '$lib/components/quick-access/CaptureMode.svelte'
  import type { VaultEntryDetail } from '$lib/types/vault'

  const win = getCurrentWindow()

  // Element refs
  let searchInputEl = $state<HTMLInputElement | null>(null)
  let unlisteners: UnlistenFn[] = []

  // Reactive state — primitive $state to avoid svelte-check edge cases.
  let mode = $state<'record' | 'search'>('record')
  let draft = $state('')
  let query = $state('')
  let selectedId = $state<string | null>(null)
  let preferences = $state<DockPreferences | null>(null)
  let systemDark = $state(true)

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

  function onOpenAiSettings() {
    // Task 17+ will wire this to open the main window's vault settings panel
    // via a Tauri command. For now we emit a no-op event so behavior remains
    // local; the quick-access window itself does not host the settings UI.
    notify('请到主窗口资料库设置中配置 AI', 'success')
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
      loadLocale(prefs.language)
      preferences = prefs
    } catch (e) {
      console.error('QuickAccessApp: failed to load preferences', e)
    }

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
            if (prefs.language && preferences?.language !== prefs.language) {
              loadLocale(prefs.language)
            }
            preferences = prefs
          })
          .catch(() => {})
      }),
    )

    // Sensitive reset on blur (window will also hide from Rust side)
    unlisteners.push(
      await listen('vault-sensitive-reset', () => {
        draft = ''
        query = ''
        selectedId = null
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
        searchInputEl?.focus()
      }
    })
  }

  function requestHide() {
    win.hide().catch((e: unknown) => console.error('hide failed:', e))
  }

  function onKeydown(e: KeyboardEvent) {
    // Build a transient QuickAccessState view for the pure helper.
    const snapshot: QuickAccessState = { mode, draft, query, selectedId }
    handleKeydown(snapshot, e, requestHide)
    // Sync back any mode change.
    if (snapshot.mode !== mode) {
      mode = snapshot.mode
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<svelte:window onkeydown={onKeydown} />

<main class="quick-shell">
  {#if noticeText}
    <div class="notice" class:error={noticeKind === 'error'}>{noticeText}</div>
  {/if}

  {#if mode === 'record'}
    <CaptureMode
      {notify}
      onSaved={onCaptureSaved}
      onOpenSettings={onOpenAiSettings}
    />
  {:else}
    <!-- Task 17: SearchMode.svelte will replace this placeholder -->
    <section class="mode mode-search">
      <header class="mode-header">
        <h2>搜索</h2>
        <span class="hint">Ctrl+Tab → 录入</span>
      </header>
      <input
        bind:this={searchInputEl}
        class="search-input"
        type="search"
        placeholder="搜索资料库…"
        bind:value={query}
      />
      <div class="search-results-placeholder">
        {#if query}
          <p>"{query}" 的结果将在这里显示</p>
        {:else}
          <p class="muted">输入关键词以开始搜索</p>
        {/if}
      </div>
    </section>
  {/if}
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

  .mode {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 0.75rem;
    gap: 0.5rem;
    min-height: 0;
  }

  .mode-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .mode-header h2 {
    margin: 0;
    font-size: var(--font-md, 15px);
    font-weight: 600;
    color: var(--text-primary);
  }

  .hint {
    font-size: var(--font-xs, 11px);
    color: var(--text-muted);
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

  .search-input {
    width: 100%;
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 6px);
    padding: 0.5rem 0.65rem;
    background: var(--surface-1);
    color: var(--text-primary);
    font-family: var(--font-family-en, 'Segoe UI'), var(--font-family-zh, 'Microsoft YaHei'),
      sans-serif;
    font-size: var(--font-md, 15px);
    outline: none;
  }

  .search-input:focus {
    border-color: var(--color-primary);
  }

  .search-results-placeholder {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-primary);
    overflow: auto;
  }

  .search-results-placeholder p {
    margin: 0;
    font-size: var(--font-sm, 13px);
  }

  .muted {
    color: var(--text-muted);
  }
</style>
