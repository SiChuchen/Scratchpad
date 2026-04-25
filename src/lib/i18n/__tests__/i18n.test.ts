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
