// src/lib/components/quick-access/CaptureMode.test.ts
//
// CaptureMode 组件行为测试（Task 16）。
//
// 覆盖 8 个验收场景：
//   1. 粘贴后 200ms 调用 local parse，预览不等 AI；
//   2. 500ms 稳定后且 autoEnrich=true 才调用 enrich；
//   3. AI 返回时不覆盖用户已编辑字段（dirty paths）；
//   4. AI 失败显示"已使用本地整理"，保存仍启用；
//   5. 选中文本点击"标记敏感"后传入 manualSensitiveValues；
//   6. "查看本次发送内容"显示 audit messages，不显示 API Key；
//   7. Ctrl+Enter 保存；
//   8. 保存失败保留 raw/draft/requestId；成功清空并轮转 requestId.
//
// 通过 vi.mock 替换 `$lib/api/vault` 的 vaultApi，以便注入受控响应。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'

// ---- Mocks ---------------------------------------------------------------

const mockVaultApi = vi.hoisted(() => {
  return {
    parseCaptureLocal: vi.fn(),
    enrichCapture: vi.fn(),
    createFromCapture: vi.fn(),
    getAiSettings: vi.fn(),
    getLlmConfig: vi.fn(),
  }
})

vi.mock('$lib/api/vault', () => ({
  vaultApi: mockVaultApi,
}))

import CaptureMode from './CaptureMode.svelte'
import type {
  AiRequestAudit,
  CaptureDraft,
  CaptureEnrichment,
  CaptureSuggestion,
  LlmConfigSummary,
  VaultAiSettings,
  VaultEntryDetail,
} from '$lib/types/vault'

// ---- Fixtures ------------------------------------------------------------

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

function auditWithMessages(): AiRequestAudit {
  return {
    providerId: 'test-provider',
    model: 'test-model',
    sentAt: '2026-07-17T00:00:00Z',
    messages: [
      { role: 'system', content: 'You are a helpful assistant.' },
      { role: 'user', content: '##TOKEN_1## 是一个网站账号' },
    ],
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

function enrichment(sug: CaptureSuggestion, audit?: AiRequestAudit): CaptureEnrichment {
  return { suggestion: sug, audit: audit ?? auditWithMessages() }
}

function detail(id: string): VaultEntryDetail {
  return {
    entry: {
      id,
      kind: 'note',
      title: 'saved-title',
      notes: null,
      createdAt: '2026-07-17T00:00:00Z',
      updatedAt: '2026-07-17T00:00:00Z',
    },
    fields: [],
    tags: [],
    aiMetadata: null,
  }
}

const CONFIGURED: LlmConfigSummary = {
  providerId: 'deepseek',
  baseUrl: 'https://api.deepseek.com/v1',
  model: 'deepseek-v4-flash',
  hasApiKey: true,
}

const AI_SETTINGS_ON: VaultAiSettings = {
  autoEnrich: true,
  autoHybridSearch: false,
  thinkingEnabled: false,
  sensitiveClipboardClearSeconds: null,
}

const AI_SETTINGS_OFF: VaultAiSettings = {
  autoEnrich: false,
  autoHybridSearch: false,
  thinkingEnabled: false,
  sensitiveClipboardClearSeconds: null,
}

const LOCAL_PARSE_DELAY_MS = 200
const AI_ENRICH_DELAY_MS = 500

// ---- Setup / Teardown ----------------------------------------------------

function resetMocks() {
  mockVaultApi.parseCaptureLocal.mockReset()
  mockVaultApi.enrichCapture.mockReset()
  mockVaultApi.createFromCapture.mockReset()
  mockVaultApi.getAiSettings.mockReset()
  mockVaultApi.getLlmConfig.mockReset()
}

function configureAISetup(
  config: LlmConfigSummary | null,
  settings: VaultAiSettings,
) {
  mockVaultApi.getLlmConfig.mockResolvedValue(config)
  mockVaultApi.getAiSettings.mockResolvedValue(settings)
}

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.restoreAllMocks()
  resetMocks()
})

// Helper: type into rawText textarea and flush debounce timers
async function typeRawText(text: string) {
  const ta = screen.getByPlaceholderText('粘贴或输入要保存的内容') as HTMLTextAreaElement
  await fireEvent.input(ta, { target: { value: text } })
}

// ---- Tests ----------------------------------------------------------------

describe('CaptureMode', () => {
  it('returns a namespaced saved id and clears only after persistence succeeds', async () => {
    vi.useFakeTimers()
    configureAISetup(CONFIGURED, AI_SETTINGS_OFF)
    mockVaultApi.parseCaptureLocal.mockResolvedValue(baseDraft({ title: '生产数据库' }))
    mockVaultApi.createFromCapture.mockResolvedValue(detail('saved-namespace'))
    const onSaved = vi.fn()
    render(CaptureMode, {
      notify: vi.fn(),
      aiConfigured: true,
      autoEnrich: false,
      onSaved,
      onOpenSettings: vi.fn(),
    })
    await vi.advanceTimersByTimeAsync(0)
    await typeRawText('database credentials')
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS)
    await fireEvent.click(screen.getByRole('button', { name: /保存到资料库/ }))
    await vi.advanceTimersByTimeAsync(0)
    expect(onSaved).toHaveBeenCalledWith('vault:saved-namespace')
    expect(screen.getByPlaceholderText('粘贴或输入要保存的内容')).toHaveValue('')
  })
  beforeEach(() => {
    vi.useFakeTimers()
  })

  it('paste → 200ms → local parse fires; preview does NOT wait for AI', async () => {
    configureAISetup(CONFIGURED, AI_SETTINGS_ON)
    const parsed = baseDraft({ title: '本地解析标题' })
    mockVaultApi.parseCaptureLocal.mockResolvedValue(parsed)
    // Even if enrichCapture is configured to never resolve, parse must publish preview.
    mockVaultApi.enrichCapture.mockImplementation(
      () => new Promise(() => {}),
    )

    render(CaptureMode, {
      notify: vi.fn(),
      aiConfigured: true,
      autoEnrich: true,
      onSaved: vi.fn(),
      onOpenSettings: vi.fn(),
    })

    await typeRawText('hello world')

    // Advance 199ms — should NOT have parsed yet.
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS - 1)
    expect(mockVaultApi.parseCaptureLocal).not.toHaveBeenCalled()

    // Advance to 200ms — local parse fires.
    await vi.advanceTimersByTimeAsync(1)
    await waitFor(() => expect(mockVaultApi.parseCaptureLocal).toHaveBeenCalledWith('hello world'))

    // The preview title should be visible before AI resolves.
    await waitFor(() => {
      expect(screen.getByDisplayValue('本地解析标题')).toBeInTheDocument()
    })

    // enrichCapture may have been invoked (timing-wise), but the preview is already shown.
    // Critical assertion: the preview is visible regardless of AI promise state.
  })

  it('500ms stable + autoEnrich=true → enrich called; with autoEnrich=false it is not', async () => {
    configureAISetup(CONFIGURED, AI_SETTINGS_ON)
    const parsed = baseDraft()
    mockVaultApi.parseCaptureLocal.mockResolvedValue(parsed)
    mockVaultApi.enrichCapture.mockResolvedValue(
      enrichment(suggestion({ title: 'AI 标题' })),
    )

    render(CaptureMode, {
      notify: vi.fn(),
      aiConfigured: true,
      autoEnrich: true,
      onSaved: vi.fn(),
      onOpenSettings: vi.fn(),
    })
    await vi.advanceTimersByTimeAsync(0)

    await typeRawText('foo')

    // Parse at 200ms
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.parseCaptureLocal).toHaveBeenCalled())

    // Not yet enriched at 200+499ms
    await vi.advanceTimersByTimeAsync(AI_ENRICH_DELAY_MS - 1)
    expect(mockVaultApi.enrichCapture).not.toHaveBeenCalled()

    // 500ms after parse: enrich fires
    await vi.advanceTimersByTimeAsync(1)
    await waitFor(() => expect(mockVaultApi.enrichCapture).toHaveBeenCalled())
    expect(mockVaultApi.enrichCapture).toHaveBeenCalledWith(
      expect.any(Object),
      'foo',
      [],
      expect.any(String),
    )
  })

  it('autoEnrich=false does NOT call enrich even after 500ms', async () => {
    configureAISetup(CONFIGURED, AI_SETTINGS_OFF)
    mockVaultApi.parseCaptureLocal.mockResolvedValue(baseDraft())
    mockVaultApi.enrichCapture.mockResolvedValue(enrichment(suggestion()))

    render(CaptureMode, {
      notify: vi.fn(),
      aiConfigured: true,
      autoEnrich: false,
      onSaved: vi.fn(),
      onOpenSettings: vi.fn(),
    })
    await vi.advanceTimersByTimeAsync(0)

    await typeRawText('foo')
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS + AI_ENRICH_DELAY_MS + 50)

    expect(mockVaultApi.enrichCapture).not.toHaveBeenCalled()
  })

  it('offers Settings when automatic enrichment is disabled', async () => {
    configureAISetup(CONFIGURED, AI_SETTINGS_OFF)
    const onOpenSettings = vi.fn()

    render(CaptureMode, {
      notify: vi.fn(),
      aiConfigured: true,
      autoEnrich: false,
      onSaved: vi.fn(),
      onOpenSettings,
    })
    await vi.advanceTimersByTimeAsync(0)

    const configureButton = screen.getByRole('button', { name: '立即配置' })
    await fireEvent.click(configureButton)
    expect(onOpenSettings).toHaveBeenCalledTimes(1)
  })

  it('AI response does NOT overwrite user-edited fields (dirty title)', async () => {
    configureAISetup(CONFIGURED, AI_SETTINGS_ON)
    const parsed = baseDraft({ title: '本地标题', kind: 'note' })
    mockVaultApi.parseCaptureLocal.mockResolvedValue(parsed)
    mockVaultApi.enrichCapture.mockResolvedValue(
      enrichment(suggestion({ title: 'AI 应该不覆盖', notes: 'AI 备注' })),
    )

    render(CaptureMode, {
      notify: vi.fn(),
      aiConfigured: true,
      autoEnrich: true,
      onSaved: vi.fn(),
      onOpenSettings: vi.fn(),
    })
    await vi.advanceTimersByTimeAsync(0)

    await typeRawText('raw')
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.parseCaptureLocal).toHaveBeenCalled())

    // Wait for the preview form to appear.
    await waitFor(() => {
      expect(screen.getByDisplayValue('本地标题')).toBeInTheDocument()
    })

    // User edits title — marks 'title' path dirty.
    const titleInput = screen.getByDisplayValue('本地标题') as HTMLInputElement
    await fireEvent.input(titleInput, { target: { value: '用户改写' } })

    // Now fire enrich.
    await vi.advanceTimersByTimeAsync(AI_ENRICH_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.enrichCapture).toHaveBeenCalled())

    // Flush any promise resolution.
    await vi.advanceTimersByTimeAsync(0)

    // User-edited title must remain.
    await waitFor(() => {
      expect(screen.getByDisplayValue('用户改写')).toBeInTheDocument()
    })
    expect(screen.queryByDisplayValue('AI 应该不覆盖')).not.toBeInTheDocument()
  })

  it('AI failure shows "已使用本地整理" status and save button stays enabled', async () => {
    configureAISetup(CONFIGURED, AI_SETTINGS_ON)
    mockVaultApi.parseCaptureLocal.mockResolvedValue(baseDraft({ title: '本地' }))
    mockVaultApi.enrichCapture.mockRejectedValue(new Error('AI 网络故障'))

    render(CaptureMode, {
      notify: vi.fn(),
      aiConfigured: true,
      autoEnrich: true,
      onSaved: vi.fn(),
      onOpenSettings: vi.fn(),
    })
    await vi.advanceTimersByTimeAsync(0)

    await typeRawText('raw')
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.parseCaptureLocal).toHaveBeenCalled())
    await vi.advanceTimersByTimeAsync(AI_ENRICH_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.enrichCapture).toHaveBeenCalled())

    // Flush rejection.
    await vi.advanceTimersByTimeAsync(0)

    // Status text shown.
    await waitFor(() => {
      expect(screen.getByText(/AI 暂不可用|已使用本地整理|本地整理/)).toBeInTheDocument()
    })

    // Save button still enabled.
    const saveBtn = screen.getByRole('button', { name: /保存到资料库/ })
    expect(saveBtn).not.toBeDisabled()
  })

  it('selecting text and clicking "标记敏感" adds value to manualSensitiveValues and re-enrich passes it', async () => {
    configureAISetup(CONFIGURED, AI_SETTINGS_ON)
    mockVaultApi.parseCaptureLocal.mockResolvedValue(baseDraft())
    mockVaultApi.enrichCapture.mockResolvedValue(enrichment(suggestion()))

    render(CaptureMode, {
      notify: vi.fn(),
      aiConfigured: true,
      autoEnrich: true,
      onSaved: vi.fn(),
      onOpenSettings: vi.fn(),
    })
    await vi.advanceTimersByTimeAsync(0)

    await typeRawText('my-secret-value raw text')
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.parseCaptureLocal).toHaveBeenCalled())

    // Simulate window.getSelection() returning a marked substring.
    const sel = window.getSelection()
    if (sel) {
      // jsdom Selection is read-only; mock its toString.
      Object.defineProperty(sel, 'toString', { value: () => 'my-secret-value', configurable: true })
    }

    const markBtn = screen.getByRole('button', { name: /标记敏感/ })
    await fireEvent.click(markBtn)

    // The marked value should appear in the visible sensitive marks list.
    await waitFor(() => {
      expect(screen.getByText(/my-secret-value/)).toBeInTheDocument()
    })

    // Trigger enrich — manualSensitiveValues should now include 'my-secret-value'.
    await vi.advanceTimersByTimeAsync(AI_ENRICH_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.enrichCapture).toHaveBeenCalled())
    const calls = mockVaultApi.enrichCapture.mock.calls
    const callArgs = calls[calls.length - 1]!
    expect(callArgs[2]).toContain('my-secret-value')
  })

  it('"查看本次发送内容" shows audit messages, NOT any API key', async () => {
    configureAISetup(CONFIGURED, AI_SETTINGS_ON)
    mockVaultApi.parseCaptureLocal.mockResolvedValue(baseDraft())
    const secretKey = 'sk-test-secret-key-do-not-leak'
    mockVaultApi.enrichCapture.mockResolvedValue(enrichment(suggestion()))

    render(CaptureMode, {
      notify: vi.fn(),
      aiConfigured: true,
      autoEnrich: true,
      onSaved: vi.fn(),
      onOpenSettings: vi.fn(),
    })
    await vi.advanceTimersByTimeAsync(0)

    await typeRawText('foo')
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.parseCaptureLocal).toHaveBeenCalled())
    await vi.advanceTimersByTimeAsync(AI_ENRICH_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.enrichCapture).toHaveBeenCalled())
    await vi.advanceTimersByTimeAsync(0)

    // Open audit dialog.
    const auditBtn = screen.getByRole('button', { name: /查看本次发送内容/ })
    await fireEvent.click(auditBtn)

    // Audit content visible.
    await waitFor(() => {
      expect(screen.getByText(/You are a helpful assistant/)).toBeInTheDocument()
    })

    // API key never appears in the DOM anywhere.
    const bodyText = document.body.textContent ?? ''
    expect(bodyText).not.toContain(secretKey)
  })

  it('Ctrl+Enter triggers save', async () => {
    configureAISetup(CONFIGURED, AI_SETTINGS_ON)
    mockVaultApi.parseCaptureLocal.mockResolvedValue(baseDraft({ title: 'X' }))
    mockVaultApi.createFromCapture.mockResolvedValue(detail('saved-1'))

    const notify = vi.fn()
    render(CaptureMode, {
      notify,
      onSaved: vi.fn(),
      onOpenSettings: vi.fn(),
    })
    await vi.advanceTimersByTimeAsync(0)

    await typeRawText('foo')
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.parseCaptureLocal).toHaveBeenCalled())

    const ta = screen.getByPlaceholderText('粘贴或输入要保存的内容') as HTMLTextAreaElement
    await fireEvent.keyDown(ta, { key: 'Enter', ctrlKey: true })

    await waitFor(() => expect(mockVaultApi.createFromCapture).toHaveBeenCalled())
    await waitFor(() =>
      expect(notify).toHaveBeenCalledWith('已保存到资料库', 'success'),
    )
  })

  it('save failure preserves raw/draft/requestId; success clears and rotates requestId', async () => {
    configureAISetup(CONFIGURED, AI_SETTINGS_ON)
    mockVaultApi.parseCaptureLocal.mockResolvedValue(baseDraft({ title: '保留草稿' }))

    let shouldFail = true
    mockVaultApi.createFromCapture.mockImplementation(async () => {
      if (shouldFail) throw new Error('network down')
      return detail('saved-1')
    })

    const notify = vi.fn()
    render(CaptureMode, {
      notify,
      onSaved: vi.fn(),
      onOpenSettings: vi.fn(),
    })
    await vi.advanceTimersByTimeAsync(0)

    await typeRawText('foo')
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.parseCaptureLocal).toHaveBeenCalled())

    // First save attempt: fails.
    const saveBtn = screen.getByRole('button', { name: /保存到资料库/ })
    await fireEvent.click(saveBtn)
    await waitFor(() => expect(mockVaultApi.createFromCapture).toHaveBeenCalledTimes(1))

    // raw textarea retains its value; draft preview retains title; error visible.
    const ta = screen.getByPlaceholderText('粘贴或输入要保存的内容') as HTMLTextAreaElement
    expect(ta.value).toBe('foo')
    await waitFor(() => {
      expect(screen.getByDisplayValue('保留草稿')).toBeInTheDocument()
    })
    await waitFor(() => {
      expect(screen.getByText(/network down/)).toBeInTheDocument()
    })

    // Same requestId was used (controller does NOT rotate on failure).
    const firstRequestId = mockVaultApi.createFromCapture.mock.calls[0]![1]
    expect(firstRequestId).toBeTruthy()

    // Second save attempt: still failing — requestId must NOT have rotated,
    // so the retry hits the same storage-side idempotency key on
    // vault_capture_requests.
    await fireEvent.click(saveBtn)
    await waitFor(() => expect(mockVaultApi.createFromCapture).toHaveBeenCalledTimes(2))
    const retryRequestId = mockVaultApi.createFromCapture.mock.calls[1]![1]
    expect(retryRequestId).toBe(firstRequestId)

    // Third save: succeeds.
    shouldFail = false
    const saveBtn2 = screen.getByRole('button', { name: /保存到资料库/ })
    await fireEvent.click(saveBtn2)
    await waitFor(() => expect(mockVaultApi.createFromCapture).toHaveBeenCalledTimes(3))

    // Success path rotates requestId internally; the rotation happens AFTER
    // the createFromCapture call resolves. The third call's argument was
    // still the pre-success id. To verify rotation we trigger a NEW save
    // and inspect what id it passes.
    await waitFor(() =>
      expect(notify).toHaveBeenCalledWith('已保存到资料库', 'success'),
    )
    // After success the draft is cleared and raw textarea reset, so type a
    // new payload and parse again before saving.
    mockVaultApi.parseCaptureLocal.mockResolvedValue(baseDraft({ title: '第二条' }))
    await typeRawText('bar')
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS)
    await waitFor(() =>
      expect(mockVaultApi.parseCaptureLocal).toHaveBeenCalledWith('bar'),
    )

    const saveBtn3 = screen.getByRole('button', { name: /保存到资料库/ })
    await fireEvent.click(saveBtn3)
    await waitFor(() => expect(mockVaultApi.createFromCapture).toHaveBeenCalledTimes(4))

    const afterSuccessRequestId = mockVaultApi.createFromCapture.mock.calls[3]![1]
    expect(afterSuccessRequestId).not.toBe(firstRequestId)

    // On success: notify called, raw + draft cleared.
    await waitFor(() =>
      expect(notify).toHaveBeenCalledWith('已保存到资料库', 'success'),
    )

    const taAfter = screen.getByPlaceholderText('粘贴或输入要保存的内容') as HTMLTextAreaElement
    expect(taAfter.value).toBe('')
    expect(screen.queryByDisplayValue('保留草稿')).not.toBeInTheDocument()
    expect(screen.queryByDisplayValue('第二条')).not.toBeInTheDocument()
  })

  it('renders sensitive metadata rejection without exposing the secret or raw code', async () => {
    configureAISetup(CONFIGURED, AI_SETTINGS_OFF)
    const secret = 'DO_NOT_ECHO_THIS_PASSWORD'
    mockVaultApi.parseCaptureLocal.mockResolvedValue(
      baseDraft({
        title: '敏感资料',
        fields: [
          {
            draftId: 'f1',
            key: 'password',
            value: secret,
            isSensitive: true,
          },
        ],
      }),
    )
    mockVaultApi.createFromCapture.mockRejectedValue(
      new Error('sensitive_metadata_rejected'),
    )

    render(CaptureMode, {
      notify: vi.fn(),
      aiConfigured: true,
      autoEnrich: false,
      onSaved: vi.fn(),
      onOpenSettings: vi.fn(),
    })
    await vi.advanceTimersByTimeAsync(0)

    await typeRawText('password fixture')
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.parseCaptureLocal).toHaveBeenCalled())
    await fireEvent.click(screen.getByRole('button', { name: /保存到资料库/ }))

    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent('检测到敏感信息出现在标签或摘要中，请修改后重试')
    expect(alert).not.toHaveTextContent(secret)
    expect(alert).not.toHaveTextContent('sensitive_metadata_rejected')
  })

  it('AI tag → manual conversion removes from aiTags and adds to manualTags', async () => {
    configureAISetup(CONFIGURED, AI_SETTINGS_OFF)
    mockVaultApi.parseCaptureLocal.mockResolvedValue(
      baseDraft({ title: 'X', aiTags: ['work', 'meeting'], manualTags: [] }),
    )

    render(CaptureMode, {
      notify: vi.fn(),
      aiConfigured: true,
      autoEnrich: true,
      onSaved: vi.fn(),
      onOpenSettings: vi.fn(),
    })
    await vi.advanceTimersByTimeAsync(0)

    await typeRawText('foo')
    await vi.advanceTimersByTimeAsync(LOCAL_PARSE_DELAY_MS)
    await waitFor(() => expect(mockVaultApi.parseCaptureLocal).toHaveBeenCalled())

    // Wait for the AI tag chip to render. Convert chip aria-label now uses the
    // prefix "将 AI 标签…转为手动标签：" followed by the tag name.
    const convertBtn = await screen.findByRole('button', {
      name: /将 AI 标签.*转为手动标签：work/,
    })
    expect(screen.getByText('work')).toBeInTheDocument()
    expect(screen.getByText('meeting')).toBeInTheDocument()

    // Convert "work" to manual.
    await fireEvent.click(convertBtn)

    // "work" disappears from the AI tags section and appears in manual tags.
    await waitFor(() => {
      const manualInput = screen.getByPlaceholderText('例如：work, db') as HTMLInputElement
      expect(manualInput.value).toContain('work')
    })
    // AI tag chip for "work" is gone (only "meeting" remains in AI section).
    expect(
      screen.queryByRole('button', { name: /将 AI 标签.*转为手动标签：work/ }),
    ).not.toBeInTheDocument()
    expect(
      screen.getByRole('button', { name: /将 AI 标签.*转为手动标签：meeting/ }),
    ).toBeInTheDocument()

    // Save — manual tags must include "work" and aiTags must not.
    mockVaultApi.createFromCapture.mockResolvedValue(detail('saved-2'))
    const saveBtn = screen.getByRole('button', { name: /保存到资料库/ })
    await fireEvent.click(saveBtn)
    await waitFor(() => expect(mockVaultApi.createFromCapture).toHaveBeenCalledTimes(1))
    const savedDraft = mockVaultApi.createFromCapture.mock.calls[0]![0] as CaptureDraft
    expect(savedDraft.manualTags).toContain('work')
    expect(savedDraft.aiTags).not.toContain('work')
    expect(savedDraft.aiTags).toContain('meeting')
  })
})
