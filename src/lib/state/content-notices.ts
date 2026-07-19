import type { LocaleMessages } from '$lib/i18n/types'

export type ContentNoticeCode =
  | 'saved'
  | 'unsaved'
  | 'deleted'
  | 'deleteFailedRestored'
  | 'undoExpired'
  | 'copyFailed'
  | 'refreshFailed'

export function resolveContentNotice(
  locale: LocaleMessages,
  code: ContentNoticeCode,
): string {
  return locale.workspace.notices[code]
}
