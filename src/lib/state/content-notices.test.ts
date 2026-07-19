import { describe, expect, it } from 'vitest'
import en from '$lib/i18n/locales/en'
import zhCN from '$lib/i18n/locales/zh-CN'
import { resolveContentNotice, type ContentNoticeCode } from './content-notices'

const codes: ContentNoticeCode[] = [
  'saved',
  'unsaved',
  'deleted',
  'deleteFailedRestored',
  'undoExpired',
  'copyFailed',
  'refreshFailed',
]

describe('resolveContentNotice', () => {
  it.each(codes)('resolves %s in both locales', (code) => {
    expect(resolveContentNotice(zhCN, code)).not.toHaveLength(0)
    expect(resolveContentNotice(en, code)).not.toHaveLength(0)
  })
})
