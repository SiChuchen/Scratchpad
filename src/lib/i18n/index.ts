import type { LocaleMessages } from './types'
import zhCN from './locales/zh-CN'
import en from './locales/en'

const locales = { 'zh-CN': zhCN, en } as const

function getInitialLocale(): 'zh-CN' | 'en' {
  if (typeof navigator !== 'undefined' && navigator.language?.startsWith('zh')) return 'zh-CN'
  return 'en'
}

function cloneLocale(lang: string): LocaleMessages {
  const locale = locales[lang as keyof typeof locales] || locales.en
  return JSON.parse(JSON.stringify(locale))
}

/** Current locale messages. Mutated by loadLocale(). */
export const messages: LocaleMessages = cloneLocale(getInitialLocale())

/** Detect language from navigator.language. Returns 'zh-CN' or 'en'. */
export function detectLanguage(): string {
  return getInitialLocale()
}

/** Incremented each time loadLocale() is called. */
export let localeVersion = 0

/** Load a locale into the messages object. */
export function loadLocale(lang: string): void {
  const locale = cloneLocale(lang)
  for (const key of Object.keys(locale) as (keyof LocaleMessages)[]) {
    ;(messages as unknown as Record<string, unknown>)[key] = locale[key]
  }
  localeVersion++
}
