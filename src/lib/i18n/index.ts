import type { LocaleMessages } from './types'
import zhCN from './locales/zh-CN'
import en from './locales/en'

const locales: Record<string, LocaleMessages> = { 'zh-CN': zhCN, en }

function getInitialLocale(): string {
  if (typeof navigator !== 'undefined' && navigator.language?.startsWith('zh')) return 'zh-CN'
  return 'en'
}

/** Current locale messages — initialized from system locale, updated via loadLocale(). */
export const messages: LocaleMessages = { ...locales[getInitialLocale()] }

/** Detect language from navigator.language. Returns 'zh-CN' or 'en'. */
export function detectLanguage(): string {
  return getInitialLocale()
}

/** Load a locale into the exported messages object. */
export function loadLocale(lang: string): void {
  const locale = locales[lang] || locales.en
  for (const key of Object.keys(locale) as (keyof LocaleMessages)[]) {
    messages[key] = locale[key]
  }
}
