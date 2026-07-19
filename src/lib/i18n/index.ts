import type { LocaleMessages } from './types'
import zhCN from './locales/zh-CN'
import en from './locales/en'
import { reactiveMessages } from './reactive-messages.svelte'

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
export const messages: LocaleMessages = reactiveMessages
Object.assign(messages, cloneLocale(getInitialLocale()))

/** Tracks the active locale code so callers can branch on language without
 *  inspecting message strings. */
let currentLocaleCode: 'zh-CN' | 'en' = getInitialLocale()

/** Detect language from navigator.language. Returns 'zh-CN' or 'en'. */
export function detectLanguage(): string {
  return getInitialLocale()
}

/** Incremented each time loadLocale() is called. */
export let localeVersion = 0

/** Load a locale into the messages object. */
export function loadLocale(lang: string): void {
  const code = lang === 'zh-CN' ? 'zh-CN' : 'en'
  currentLocaleCode = code
  const locale = cloneLocale(lang)
  for (const key of Object.keys(locale) as (keyof LocaleMessages)[]) {
    ;(messages as unknown as Record<string, unknown>)[key] = locale[key]
  }
  localeVersion++
}

/** Returns true when the active locale is Simplified Chinese. Components should
 *  prefer this helper over brittle pattern-matching on message strings. */
export function isZh(): boolean {
  return currentLocaleCode === 'zh-CN'
}
