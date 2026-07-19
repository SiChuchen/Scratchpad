import { describe, it, expect } from 'vitest'
import zhCN from '../locales/zh-CN'
import en from '../locales/en'

/** Recursively collect all leaf key paths from a nested object. */
function collectKeys(obj: Record<string, unknown>, prefix = ''): string[] {
  const keys: string[] = []
  for (const [key, value] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${key}` : key
    if (typeof value === 'object' && value !== null && !Array.isArray(value)) {
      keys.push(...collectKeys(value as Record<string, unknown>, path))
    } else {
      keys.push(path)
    }
  }
  return keys
}

describe('i18n dictionaries', () => {
  it('zh-CN and en have identical key structure', () => {
    const zhKeys = collectKeys(zhCN as unknown as Record<string, unknown>).sort()
    const enKeys = collectKeys(en as unknown as Record<string, unknown>).sort()
    expect(enKeys).toEqual(zhKeys)
  })

  it('contains no user-visible legacy vault name', () => {
    expect(JSON.stringify(zhCN)).not.toContain('保险箱')
    expect(JSON.stringify(en)).not.toMatch(/\bvault\b/i)
  })

  it('all string values are non-empty', () => {
    const checkNonEmpty = (obj: Record<string, unknown>) => {
      for (const [key, value] of Object.entries(obj)) {
        if (typeof value === 'string') {
          expect(value.length, `Empty value for key "${key}"`).toBeGreaterThan(0)
        } else if (typeof value === 'object' && value !== null) {
          checkNonEmpty(value as Record<string, unknown>)
        }
      }
    }
    checkNonEmpty(zhCN as unknown as Record<string, unknown>)
    checkNonEmpty(en as unknown as Record<string, unknown>)
  })

  it('expert labels cover all expected tokens', () => {
    const expertKeys = Object.keys(zhCN.expert)
    expect(expertKeys.length).toBeGreaterThanOrEqual(24)
    expect(Object.keys(en.expert).length).toBe(expertKeys.length)
  })

  it('contains English recovery and Library workflow messages', () => {
    expect(en.quickAccess.aiNotConfigured).toBe('AI is not configured; using local organization only')
    expect(en.quickAccess.autoEnrichDisabled).toBe('AI auto-organization is off')
    expect(en.quickAccess.configureNow).toBe('Configure now')
    expect(en.quickAccess.openSettingsFailed).toBe('Could not open main window settings')
    expect(zhCN.quickAccess.openFailed).toBe('无法打开快速入口')
    expect(en.quickAccess.openFailed).toBe('Could not open quick access')
    expect(zhCN.quickAccess.usefulInformation).toBe('可直接使用的信息')
    expect(en.quickAccess.usefulInformation).toBe('Information to use')
    expect(en.settings.selectDataDirTitle).toBe('Select data directory')
    expect(en.settings.changeDataDirFailed).toBe('Could not change data directory')
    expect(en.library.saved).toBe('Saved')
    expect(en.library.openQuickAccess).toBe('Open quick access')
  })

  it('does not expose library as a separate destination', () => {
    for (const locale of [zhCN, en]) {
      expect('library' in locale.nav).toBe(false)
      expect(JSON.stringify(locale.nav)).not.toContain('资料库')
      expect(JSON.stringify(locale.nav)).not.toContain('Library')
    }
  })

  it.each([zhCN, en])('contains the unified workspace language', (locale) => {
    expect(locale.workspace.scope.temporary).toBeTruthy()
    expect(locale.workspace.scope.all).toBeTruthy()
    expect(locale.workspace.scope.saved).toBeTruthy()
    expect(locale.workspace.searchPlaceholder).toBeTruthy()
    expect(locale.workspace.temporaryRetention).toBeTruthy()
  })
})

describe('detectLanguage', () => {
  it('returns a valid locale string', async () => {
    const { detectLanguage } = await import('../index')
    expect(typeof detectLanguage()).toBe('string')
    expect(['zh-CN', 'en']).toContain(detectLanguage())
  })
})

describe('loadLocale', () => {
  it('loads en locale into messages', async () => {
    const { messages, loadLocale } = await import('../index')
    loadLocale('en')
    expect(messages.nav.home).toBe('Dock')
    expect(messages.settings.back).toBe('← Back')
  })

  it('loads zh-CN locale back', async () => {
    const { messages, loadLocale } = await import('../index')
    loadLocale('zh-CN')
    expect(messages.nav.home).toBe('收纳')
  })

  it('falls back to en for unknown locale', async () => {
    const { messages, loadLocale } = await import('../index')
    loadLocale('fr')
    expect(messages.nav.home).toBe('Dock')
    // Restore
    loadLocale('zh-CN')
  })
})
