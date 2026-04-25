import type { LocaleMessages } from './types'
import zhCN from './locales/zh-CN'
import en from './locales/en'

const locales = { 'zh-CN': zhCN, en } as const

function getInitialLocale(): 'zh-CN' | 'en' {
  if (typeof navigator !== 'undefined' && navigator.language?.startsWith('zh')) return 'zh-CN'
  return 'en'
}

const initial = getInitialLocale()

/** Current locale messages — initialized from system locale, updated via loadLocale(). */
export const messages: LocaleMessages = JSON.parse(JSON.stringify(locales[initial]))

/** Detect language from navigator.language. Returns 'zh-CN' or 'en'. */
export function detectLanguage(): string {
  return getInitialLocale()
}

/** Load a locale into the exported messages object. */
export function loadLocale(lang: string): void {
  const locale: LocaleMessages = locales[lang as keyof typeof locales] || locales.en
  for (const key of Object.keys(locale) as (keyof LocaleMessages)[]) {
    ;(messages as unknown as Record<string, unknown>)[key] = locale[key]
  }
}
