// src/lib/state/capture-draft.test.ts
//
// CaptureDraftController 行为测试。
//
// 覆盖：
//   * AI 建议覆盖未编辑（非 dirty）的 title；
//   * AI 建议不覆盖 dirty 的 title / notes / field value；
//   * AI 新字段可以追加；
//   * 同 key 的 AI 字段不会复制出第二行（去重）；
//   * save 失败后 request ID 不变；save 成功后才生成新 session。
//
// Controller 不直接调 LLM；它只是合并 enrichment 到 draft，并暴露 save()。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  CaptureDraftController,
  type CaptureDraftApi,
} from './capture-draft'
import type {
  AiRequestAudit,
  CaptureDraft,
  CaptureEnrichment,
  CaptureSuggestion,
  EntryKind,
  VaultEntryDetail,
} from '$lib/types/vault'

// ---- helpers --------------------------------------------------------------

function baseDraft(overrides: Partial<CaptureDraft> = {}): CaptureDraft {
  return {
    kind: 'note',
    title: '本地标题',
    notes: null,
    fields: [],
    manualTags: [],
    aiTags: [],
    aiSummary: null,
    searchAliases: [],
    aiProvenance: null,
    warnings: [],
    ...overrides,
  }
}

function audit(): AiRequestAudit {
  return {
    providerId: 'test',
    model: 'test',
    sentAt: '2026-07-17T00:00:00Z',
    messages: [],
  }
}

function suggestion(overrides: Partial<CaptureSuggestion> = {}): CaptureSuggestion {
  return {
    kind: null,
    title: null,
    notes: null,
    fields: [],
    aiTags: [],
    aiSummary: null,
    searchAliases: [],
    ...overrides,
  }
}

function enrichment(sug: CaptureSuggestion): CaptureEnrichment {
  return { suggestion: sug, audit: audit() }
}

function detail(id: string): VaultEntryDetail {
  return {
    entry: {
      id,
      kind: 'note' as EntryKind,
      title: 'saved',
      notes: null,
      createdAt: '2026-07-17T00:00:00Z',
      updatedAt: '2026-07-17T00:00:00Z',
    },
    fields: [],
    tags: [],
    aiMetadata: null,
  }
}

interface ApiSpy {
  enrichCapture: ReturnType<typeof vi.fn>
  createFromCapture: ReturnType<typeof vi.fn>
}

function makeApi(
  enrichmentByQuery: Record<string, CaptureEnrichment> = {},
  saveImpl: (finalDraft: CaptureDraft, requestId: string) => Promise<VaultEntryDetail> = async () => detail('saved-1'),
): { api: CaptureDraftApi; spy: ApiSpy } {
  const spy: ApiSpy = {
    enrichCapture: vi.fn(async (
      _draft: CaptureDraft,
      rawText: string,
      _manual: string[],
      _requestId: string,
    ) => {
      const enr = enrichmentByQuery[rawText]
      if (!enr) throw new Error('no enrichment fixture for: ' + rawText)
      return enr
    }),
    createFromCapture: vi.fn(saveImpl),
  }
  return { api: spy as unknown as CaptureDraftApi, spy }
}

// ---- tests ----------------------------------------------------------------

describe('CaptureDraftController', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('AI suggestion overwrites unedited (clean) title', async () => {
    const { api, spy } = makeApi({
      'raw-1': enrichment(suggestion({ title: 'AI 标题' })),
    })
    const ctrl = new CaptureDraftController({ api })
    ctrl.startSession()
    ctrl.setLocalDraft(baseDraft({ title: '本地标题' }))

    await ctrl.enrich('raw-1', [])

    expect(ctrl.draft.title).toBe('AI 标题')
    expect(spy.enrichCapture).toHaveBeenCalledTimes(1)
  })

  it('AI suggestion does not overwrite dirty title/notes/field value', async () => {
    const { api } = makeApi({
      'raw-2': enrichment(
        suggestion({
          title: 'AI 不应覆盖',
          notes: 'AI 不应覆盖的备注',
          fields: [
            { key: 'user', value: 'AI 不应覆盖', isSensitive: false },
          ],
        }),
      ),
    })
    const ctrl = new CaptureDraftController({ api })
    ctrl.startSession()
    ctrl.setLocalDraft(
      baseDraft({
        title: '本地标题',
        notes: '本地备注',
        fields: [
          { draftId: 'f1', key: 'user', value: '本地用户名', isSensitive: false },
        ],
      }),
    )

    // Mark paths dirty by editing
    ctrl.setTitle('本地标题 - 编辑')
    ctrl.setNotes('本地备注 - 编辑')
    ctrl.setFieldValue('f1', '本地用户名 - 编辑')

    await ctrl.enrich('raw-2', [])

    expect(ctrl.draft.title).toBe('本地标题 - 编辑')
    expect(ctrl.draft.notes).toBe('本地备注 - 编辑')
    expect(ctrl.draft.fields[0].value).toBe('本地用户名 - 编辑')
  })

  it('AI new fields are appended', async () => {
    const { api } = makeApi({
      'raw-3': enrichment(
        suggestion({
          fields: [
            { key: 'host', value: 'example.com', isSensitive: false },
          ],
        }),
      ),
    })
    const ctrl = new CaptureDraftController({ api })
    ctrl.startSession()
    ctrl.setLocalDraft(
      baseDraft({
        fields: [
          { draftId: 'f1', key: 'user', value: 'admin', isSensitive: false },
        ],
      }),
    )

    await ctrl.enrich('raw-3', [])

    expect(ctrl.draft.fields).toHaveLength(2)
    expect(ctrl.draft.fields.map((f) => f.key)).toContain('host')
  })

  it('AI field with duplicate key does not add a second row', async () => {
    const { api } = makeApi({
      'raw-4': enrichment(
        suggestion({
          fields: [
            { key: 'user', value: 'AI value', isSensitive: false },
          ],
        }),
      ),
    })
    const ctrl = new CaptureDraftController({ api })
    ctrl.startSession()
    ctrl.setLocalDraft(
      baseDraft({
        fields: [
          { draftId: 'f1', key: 'user', value: 'local-user', isSensitive: false },
        ],
      }),
    )

    await ctrl.enrich('raw-4', [])

    // Existing 'user' field is not duplicated; since user value is clean (not dirty),
    // AI value should overwrite the existing one.
    const users = ctrl.draft.fields.filter((f) => f.key === 'user')
    expect(users).toHaveLength(1)
    expect(users[0].value).toBe('AI value')
  })

  it('save failure keeps request id; success generates new session', async () => {
    let saveShouldFail = true
    const { api, spy } = makeApi({}, async (d, rid) => {
      if (saveShouldFail) throw new Error('network down')
      return detail(rid)
    })

    const ctrl = new CaptureDraftController({ api })
    ctrl.startSession()
    const firstRequestId = ctrl.requestId
    expect(firstRequestId).toBeTruthy()

    ctrl.setLocalDraft(baseDraft({ title: 'hello' }))

    // First save: fails
    await expect(ctrl.save()).rejects.toThrow('network down')
    expect(spy.createFromCapture).toHaveBeenCalledTimes(1)
    expect(ctrl.requestId).toBe(firstRequestId) // unchanged after failure

    // Second save: succeeds
    saveShouldFail = false
    await ctrl.save()
    expect(spy.createFromCapture).toHaveBeenCalledTimes(2)
    // After successful save, a new session id is generated.
    expect(ctrl.requestId).not.toBe(firstRequestId)
    expect(ctrl.requestId).toBeTruthy()
  })
})
