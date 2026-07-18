// src/lib/components/quick-access/SearchMode.test.ts
//
// SearchMode 组件行为测试（Task 17）。
//
// 覆盖 9 个验收场景：
//   1. query 输入 → local hits 立即显示；
//   2. AI 状态：plan 返回后显示"AI 已理解：…"；
//   3. ArrowDown / ArrowUp 改 selectedId；
//   4. AI 更新列表后仍保留原 selectedId；
//   5. selected 消失时选择第一条；
//   6. 右栏加载 selected detail（通过 getEntry）；
//   7. 每个 title/note/tag/field 都能独立 copy；
//   8. window blur / resetToken 变化后敏感字段重新掩码；
//   9. 连续复制不关闭面板（toast 只显示字段名）。
//
// 通过 vi.mock 替换 `$lib/api/vault` 的 vaultApi，注入受控响应。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'

// ---- Clipboard mock ------------------------------------------------------

// jsdom doesn't implement navigator.clipboard; install a stub before tests.
const clipboardWriteText = vi.fn(() => Promise.resolve())
Object.defineProperty(globalThis.navigator, 'clipboard', {
  value: { writeText: clipboardWriteText, readText: () => Promise.resolve('') },
  configurable: true,
  writable: true,
})

// ---- Mocks ---------------------------------------------------------------

const mockVaultApi = vi.hoisted(() => {
  return {
    searchLocal: vi.fn(),
    planSearch: vi.fn(),
    cancelSearch: vi.fn(),
    getEntry: vi.fn(),
    getLlmConfig: vi.fn(),
    getAiSettings: vi.fn(),
    // Task 18: copy 通过 IPC 命令；测试里走 mock。
    copyText: vi.fn(() => Promise.resolve()),
  }
})

vi.mock('$lib/api/vault', () => ({
  vaultApi: mockVaultApi,
}))

import SearchMode from './SearchMode.svelte'
import type {
  AiQueryPlan,
  EntryKind,
  PlannedSearch,
  VaultEntryDetail,
  VaultEntrySummary,
  VaultSearchHit,
} from '$lib/types/vault'

// ---- Fixtures ------------------------------------------------------------

/** Build a summary with a custom title. */
function summarytitled(id: string, title: string): VaultEntrySummary {
  return {
    entry: {
      id,
      kind: 'note' as EntryKind,
      title,
      notes: null,
      createdAt: '2026-07-17T00:00:00Z',
      updatedAt: '2026-07-17T00:00:00Z',
    },
    tags: [],
    preview: null,
  }
}

function makeHit(id: string, title?: string): VaultSearchHit {
  return {
    summary: title ? summarytitled(id, title) : summarytitled(id, `entry-${id}`),
    score: 1.0,
    sources: ['local'],
  }
}

function emptyPlan(): AiQueryPlan {
  return { kinds: [], keywords: [], aliases: [], dateFrom: null, dateTo: null }
}

function makePlanned(plan: AiQueryPlan, understood: string[]): PlannedSearch {
  return {
    plan,
    understoodTerms: understood,
    audit: {
      providerId: 'test',
      model: 'test',
      sentAt: '2026-07-17T00:00:00Z',
      messages: [],
    },
  }
}

function detail(id: string, overrides: Partial<VaultEntryDetail> = {}): VaultEntryDetail {
  return {
    entry: {
      id,
      kind: 'note',
      title: `entry-${id}`,
      notes: null,
      createdAt: '2026-07-17T00:00:00Z',
      updatedAt: '2026-07-17T00:00:00Z',
    },
    fields: [],
    tags: [],
    aiMetadata: null,
    ...overrides,
  }
}

// Constants must match SearchMode.svelte
const SEARCH_DEBOUNCE_MS = 300
const AI_PLAN_DELAY_MS = 700

// ---- Setup / Teardown ----------------------------------------------------

function resetMocks() {
  mockVaultApi.searchLocal.mockReset()
  mockVaultApi.planSearch.mockReset()
  mockVaultApi.cancelSearch.mockReset()
  mockVaultApi.getEntry.mockReset()
  mockVaultApi.getLlmConfig.mockReset()
  mockVaultApi.getAiSettings.mockReset()
  mockVaultApi.copyText.mockReset()
  mockVaultApi.copyText.mockImplementation(() => Promise.resolve())
  clipboardWriteText.mockReset()
  clipboardWriteText.mockImplementation(() => Promise.resolve())
}

function configureAI(configured: boolean, autoHybrid: boolean) {
  mockVaultApi.getLlmConfig.mockResolvedValue(
    configured
      ? { providerId: 'deepseek', baseUrl: 'x', model: 'm', hasApiKey: true }
      : null,
  )
  mockVaultApi.getAiSettings.mockResolvedValue({
    autoEnrich: false,
    autoHybridSearch: autoHybrid,
    sensitiveClipboardClearSeconds: null,
  })
}

afterEach(() => {
  // Ensure cancelSearch returns a resolved promise before cleanup runs the
  // controller's dispose(); otherwise `void undefined.catch()` throws an
  // unhandled rejection.
  mockVaultApi.cancelSearch.mockImplementation(() => Promise.resolve())
  cleanup()
  vi.useRealTimers()
  vi.restoreAllMocks()
  resetMocks()
})

// Helper: type into search input.
async function typeQuery(text: string) {
  const input = screen.getByPlaceholderText('描述你要找的资料') as HTMLInputElement
  await fireEvent.input(input, { target: { value: text } })
}

// ---- Tests ---------------------------------------------------------------

describe('SearchMode', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  it('query input → local hits appear immediately (no AI wait)', async () => {
    configureAI(true, false) // autoHybridSearch=false → searchLocalOnly path
    const hits = [makeHit('a', 'Alpha 条目'), makeHit('b', 'Beta 条目')]
    mockVaultApi.searchLocal.mockResolvedValue(hits)

    render(SearchMode, {
      notify: vi.fn(),
      autoHybridSearch: false,
    })

    // Let onMount finish.
    await vi.advanceTimersByTimeAsync(0)

    await typeQuery('a')

    // Advance the debounce window — local hits must render.
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
    await waitFor(() => expect(mockVaultApi.searchLocal).toHaveBeenCalled())

    await waitFor(() => {
      expect(screen.getByText('Alpha 条目')).toBeInTheDocument()
      expect(screen.getByText('Beta 条目')).toBeInTheDocument()
    })
  })

  it('AI status shows "AI 已理解：…" after plan returns', async () => {
    configureAI(true, true)
    mockVaultApi.searchLocal.mockResolvedValue([])
    mockVaultApi.planSearch.mockResolvedValue(
      makePlanned(emptyPlan(), ['账户', '密码']),
    )

    render(SearchMode, {
      notify: vi.fn(),
      autoHybridSearch: true,
    })

    await vi.advanceTimersByTimeAsync(0)

    await typeQuery('foo')

    // Through debounce + AI delay, planSearch should resolve.
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS + AI_PLAN_DELAY_MS + 50)
    await waitFor(() => expect(mockVaultApi.planSearch).toHaveBeenCalled())

    await waitFor(() => {
      expect(screen.getByText(/AI 已理解/)).toBeInTheDocument()
    })
    expect(screen.getByText(/账户/)).toBeInTheDocument()
    expect(screen.getByText(/密码/)).toBeInTheDocument()
  })

  it('ArrowDown / ArrowUp changes selectedId', async () => {
    configureAI(true, false)
    const hits = [makeHit('a', 'A'), makeHit('b', 'B'), makeHit('c', 'C')]
    mockVaultApi.searchLocal.mockResolvedValue(hits)
    mockVaultApi.getEntry.mockImplementation(async (id: string) => detail(id))

    render(SearchMode, {
      notify: vi.fn(),
      autoHybridSearch: false,
    })

    await vi.advanceTimersByTimeAsync(0)

    await typeQuery('x')
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
    await waitFor(() => expect(mockVaultApi.searchLocal).toHaveBeenCalled())

    // First hit (a) should be initially selected.
    await waitFor(() => {
      const opts = screen.getAllByRole('option')
      expect(opts.length).toBe(3)
      expect(opts[0]!.getAttribute('aria-selected')).toBe('true')
    })

    const input = screen.getByPlaceholderText('描述你要找的资料')
    await fireEvent.keyDown(input, { key: 'ArrowDown' })

    await waitFor(() => {
      const opts = screen.getAllByRole('option')
      expect(opts[1]!.getAttribute('aria-selected')).toBe('true')
      expect(opts[0]!.getAttribute('aria-selected')).toBe('false')
    })

    await fireEvent.keyDown(input, { key: 'ArrowUp' })

    await waitFor(() => {
      const opts = screen.getAllByRole('option')
      expect(opts[0]!.getAttribute('aria-selected')).toBe('true')
    })
  })

  it('AI list update preserves original selectedId', async () => {
    configureAI(true, true)
    // Local: a, b (a is selected first)
    const localHits = [makeHit('a', 'A'), makeHit('b', 'B')]
    // Expanded: a still present
    const expandedHits = [
      { summary: summarytitled('a', 'A'), score: 1.0, sources: ['aiExpanded'] as const },
    ]

    mockVaultApi.searchLocal.mockImplementation(async (_q, plan) => {
      if (plan === null) return localHits
      return expandedHits
    })
    mockVaultApi.planSearch.mockResolvedValue(makePlanned(emptyPlan(), ['k']))
    mockVaultApi.getEntry.mockImplementation(async (id: string) => detail(id))

    render(SearchMode, {
      notify: vi.fn(),
      autoHybridSearch: true,
    })

    await vi.advanceTimersByTimeAsync(0)

    await typeQuery('q')
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
    await waitFor(() => expect(mockVaultApi.searchLocal).toHaveBeenCalled())

    // Initial selection = 'a' (first).
    await waitFor(() => {
      const opts = screen.getAllByRole('option')
      expect(opts[0]!.getAttribute('aria-selected')).toBe('true')
    })

    // Now advance past AI delay — expanded list published.
    await vi.advanceTimersByTimeAsync(AI_PLAN_DELAY_MS + 50)
    await waitFor(() => expect(mockVaultApi.planSearch).toHaveBeenCalled())

    // 'a' still in list → still selected.
    await waitFor(() => {
      const opts = screen.getAllByRole('option')
      // Find the option whose text contains 'A'
      const aOpt = opts.find((o) => o.textContent?.includes('A'))
      expect(aOpt).toBeTruthy()
      expect(aOpt!.getAttribute('aria-selected')).toBe('true')
    })
  })

  it('selectedId disappears from results → first hit is selected', async () => {
    configureAI(true, true)
    const localHits = [makeHit('a', 'A')]
    const expandedHits = [makeHit('b', 'B')]

    mockVaultApi.searchLocal.mockImplementation(async (_q, plan) => {
      if (plan === null) return localHits
      return expandedHits
    })
    mockVaultApi.planSearch.mockResolvedValue(makePlanned(emptyPlan(), ['k']))
    mockVaultApi.getEntry.mockImplementation(async (id: string) => detail(id))

    render(SearchMode, {
      notify: vi.fn(),
      autoHybridSearch: true,
    })

    await vi.advanceTimersByTimeAsync(0)

    await typeQuery('q')
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
    await waitFor(() => expect(mockVaultApi.searchLocal).toHaveBeenCalled())

    await waitFor(() => {
      expect(screen.getByText('A')).toBeInTheDocument()
    })

    // Trigger AI expand → list changes to only 'b'.
    await vi.advanceTimersByTimeAsync(AI_PLAN_DELAY_MS + 50)
    await waitFor(() => expect(mockVaultApi.planSearch).toHaveBeenCalled())

    await waitFor(() => {
      expect(screen.getByText('B')).toBeInTheDocument()
      const opts = screen.getAllByRole('option')
      expect(opts.length).toBe(1)
      expect(opts[0]!.getAttribute('aria-selected')).toBe('true')
    })
  })

  it('right pane loads selected detail via getEntry()', async () => {
    configureAI(true, false)
    const hits = [makeHit('a', 'A')]
    mockVaultApi.searchLocal.mockResolvedValue(hits)
    const det = detail('a', {
      entry: { id: 'a', kind: 'note', title: 'A', notes: '备注内容', createdAt: '', updatedAt: '' },
      fields: [
        { id: 'f1', entryId: 'a', key: '用户名', value: 'alice', isSensitive: false, sortOrder: 0 },
      ],
    })
    mockVaultApi.getEntry.mockResolvedValue(det)

    render(SearchMode, {
      notify: vi.fn(),
      autoHybridSearch: false,
    })

    await vi.advanceTimersByTimeAsync(0)

    await typeQuery('a')
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
    await waitFor(() => expect(mockVaultApi.searchLocal).toHaveBeenCalled())

    // First hit auto-selected → getEntry called for 'a'.
    await waitFor(() => {
      expect(mockVaultApi.getEntry).toHaveBeenCalledWith('a')
    })

    // The detail view should appear with the field key.
    await waitFor(() => {
      expect(screen.getByText('用户名')).toBeInTheDocument()
    })
  })

  it('puts sorted usable fields before notes and tags with prominent copy actions', async () => {
    configureAI(true, false)
    mockVaultApi.searchLocal.mockResolvedValue([makeHit('a', 'A')])
    mockVaultApi.getEntry.mockResolvedValue(detail('a', {
      entry: {
        id: 'a',
        kind: 'credential',
        title: 'GitHub 工作账号',
        notes: '公司开发账号',
        createdAt: '',
        updatedAt: '',
      },
      tags: [{ tag: '工作', normalizedTag: '工作', source: 'manual' }],
      fields: [
        { id: 'f-url', entryId: 'a', key: '网址', value: 'example.test', isSensitive: false, sortOrder: 2 },
        { id: 'f-password', entryId: 'a', key: '密码', value: 'secret', isSensitive: true, sortOrder: 1 },
        { id: 'f-account', entryId: 'a', key: '账号', value: 'alice', isSensitive: false, sortOrder: 0 },
      ],
    }))

    render(SearchMode, { notify: vi.fn(), autoHybridSearch: false })
    await vi.advanceTimersByTimeAsync(0)
    await typeQuery('github')
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
    await waitFor(() => expect(screen.getByText('账号')).toBeInTheDocument())

    const ordered = ['标题', '账号', '密码', '网址', '备注', '手动标签'].map(
      (label) => screen.getByText(label),
    )
    for (let index = 0; index < ordered.length - 1; index += 1) {
      expect(
        ordered[index]!.compareDocumentPosition(ordered[index + 1]!)
          & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy()
    }

    for (const label of ['标题', '账号', '密码', '网址']) {
      expect(screen.getByRole('button', { name: `复制 ${label}` })).toHaveTextContent('复制')
    }
    const passwordCopy = screen.getByRole('button', { name: '复制 密码' })
    expect(passwordCopy.parentElement?.lastElementChild).toBe(passwordCopy)
  })

  it('each title/notes/tag/field triggers independent copy', async () => {
    configureAI(true, false)
    const hits = [makeHit('a', 'A')]
    mockVaultApi.searchLocal.mockResolvedValue(hits)
    const det = detail('a', {
      entry: { id: 'a', kind: 'note', title: 'A 标题', notes: '一些备注', createdAt: '', updatedAt: '' },
      tags: [{ tag: '工作', normalizedTag: '工作', source: 'manual' }],
      fields: [
        { id: 'f1', entryId: 'a', key: 'API', value: 'token-xyz', isSensitive: true, sortOrder: 0 },
      ],
    })
    mockVaultApi.getEntry.mockResolvedValue(det)

    const notify = vi.fn()
    render(SearchMode, {
      notify,
      autoHybridSearch: false,
    })

    await vi.advanceTimersByTimeAsync(0)

    await typeQuery('a')
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
    await waitFor(() => expect(mockVaultApi.searchLocal).toHaveBeenCalled())
    await waitFor(() => expect(mockVaultApi.getEntry).toHaveBeenCalled())

    // Wait for detail rows to render.
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /复制 标题/ })).toBeInTheDocument()
    })

    // Copy each independently.
    await fireEvent.click(screen.getByRole('button', { name: /复制 标题/ }))
    await fireEvent.click(screen.getByRole('button', { name: /复制 备注/ }))
    await fireEvent.click(screen.getByRole('button', { name: /复制 手动标签/ }));
    await fireEvent.click(screen.getByRole('button', { name: /复制 API/ }))

    // Each notify call should include a distinct field label.
    const calls = notify.mock.calls.map((c) => c[0])
    expect(calls).toContain('已复制：标题')
    expect(calls).toContain('已复制：备注')
    // Tag copy now includes the tag value, e.g. "已复制：手动标签：工作".
    expect(calls).toContain('已复制：手动标签：工作')
    expect(calls).toContain('已复制：API')
  })

  it('window blur and resetToken change re-masks sensitive fields', async () => {
    configureAI(true, false)
    const hits = [makeHit('a', 'A')]
    mockVaultApi.searchLocal.mockResolvedValue(hits)
    const secretValue = 'super-secret-123'
    const det = detail('a', {
      entry: { id: 'a', kind: 'note', title: 'A', notes: null, createdAt: '', updatedAt: '' },
      fields: [
        { id: 'f1', entryId: 'a', key: '密码', value: secretValue, isSensitive: true, sortOrder: 0 },
      ],
    })
    mockVaultApi.getEntry.mockResolvedValue(det)

    const { rerender } = render(SearchMode, {
      notify: vi.fn(),
      resetToken: 0,
      autoHybridSearch: false,
    })

    await vi.advanceTimersByTimeAsync(0)

    await typeQuery('a')
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
    await waitFor(() => expect(mockVaultApi.getEntry).toHaveBeenCalled())

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /复制 密码/ })).toBeInTheDocument()
    })

    // Initially masked.
    expect(screen.queryByText(secretValue)).not.toBeInTheDocument()

    // Reveal via eye button.
    await fireEvent.click(screen.getByRole('button', { name: '显示 密码' }))
    await waitFor(() => expect(screen.getByText(secretValue)).toBeInTheDocument())

    // Window blur → re-mask.
    window.dispatchEvent(new Event('blur'))
    // Flush Svelte effect + microtasks (fake timers — don't use real setTimeout).
    await vi.advanceTimersByTimeAsync(0)
    await waitFor(() => {
      expect(screen.queryByText(secretValue)).not.toBeInTheDocument()
    })

    // Reveal again, then resetToken change → re-mask.
    await fireEvent.click(screen.getByRole('button', { name: '显示 密码' }))
    await waitFor(() => expect(screen.getByText(secretValue)).toBeInTheDocument())

    await rerender({ notify: vi.fn(), resetToken: 1, autoHybridSearch: false })
    await waitFor(() => {
      expect(screen.queryByText(secretValue)).not.toBeInTheDocument()
    })
  })

  it('continuous copy does not close the panel (toast only)', async () => {
    configureAI(true, false)
    const hits = [makeHit('a', 'A')]
    mockVaultApi.searchLocal.mockResolvedValue(hits)
    const det = detail('a', {
      entry: { id: 'a', kind: 'note', title: 'A 标题', notes: '备注一', createdAt: '', updatedAt: '' },
      fields: [
        { id: 'f1', entryId: 'a', key: 'API', value: 'val1', isSensitive: false, sortOrder: 0 },
        { id: 'f2', entryId: 'a', key: 'URL', value: 'val2', isSensitive: false, sortOrder: 1 },
      ],
    })
    mockVaultApi.getEntry.mockResolvedValue(det)

    const notify = vi.fn()
    render(SearchMode, {
      notify,
      autoHybridSearch: false,
    })

    await vi.advanceTimersByTimeAsync(0)

    await typeQuery('a')
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS)
    await waitFor(() => expect(mockVaultApi.getEntry).toHaveBeenCalled())

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /复制 API/ })).toBeInTheDocument()
    })

    // Copy multiple fields in sequence.
    await fireEvent.click(screen.getByRole('button', { name: /复制 API/ }))
    await fireEvent.click(screen.getByRole('button', { name: /复制 URL/ }))
    await fireEvent.click(screen.getByRole('button', { name: /复制 标题/ }))

    // Panel still shows all rows (i.e. it didn't unmount).
    expect(screen.getByRole('button', { name: /复制 API/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /复制 URL/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /复制 标题/ })).toBeInTheDocument()

    // Three notify calls (one per copy).
    expect(notify.mock.calls.length).toBeGreaterThanOrEqual(3)
  })
})
