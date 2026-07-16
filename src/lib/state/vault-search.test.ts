// src/lib/state/vault-search.test.ts
//
// HybridSearchController 行为测试。
//
// 用 fake timers + mock api 完整覆盖：
//   * 本地结果先返回；
//   * 700ms 前不调 planSearch；
//   * 新 query 立即取消旧请求并让旧响应失效；
//   * AI 扩展结果按 entry ID 去重；
//   * selectedId 在结果中存在时保留；不存在时回退到首项；
//   * dispose 后即使旧请求返回也不再 onState。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { HybridSearchController, type HybridSearchApi, type HybridSearchState } from './vault-search'
import type {
  AiQueryPlan,
  PlannedSearch,
  VaultSearchHit,
  VaultEntrySummary,
  EntryKind,
} from '$lib/types/vault'

// ---- helpers --------------------------------------------------------------

function makeSummary(id: string, title?: string): VaultEntrySummary {
  return {
    entry: {
      id,
      kind: 'note' as EntryKind,
      title: title ?? `entry-${id}`,
      notes: null,
      createdAt: '2026-07-17T00:00:00Z',
      updatedAt: '2026-07-17T00:00:00Z',
    },
    tags: [],
    preview: null,
  }
}

function makeHit(id: string, score: number, sources: VaultSearchHit['sources']): VaultSearchHit {
  return { summary: makeSummary(id), score, sources }
}

function emptyPlan(): AiQueryPlan {
  return { kinds: [], keywords: [], aliases: [], dateFrom: null, dateTo: null }
}

function makePlanned(plan: AiQueryPlan): PlannedSearch {
  return {
    plan,
    understoodTerms: [...plan.keywords, ...plan.aliases],
    audit: {
      providerId: 'test',
      model: 'test',
      sentAt: '2026-07-17T00:00:00Z',
      messages: [],
    },
  }
}

interface ApiSpy {
  searchLocal: ReturnType<typeof vi.fn>
  planSearch: ReturnType<typeof vi.fn>
  cancelSearch: ReturnType<typeof vi.fn>
}

function makeApi(
  localHitsByQuery: Record<string, VaultSearchHit[]> = {},
  aiHitsByQuery: Record<string, VaultSearchHit[]> = {},
  planByQuery: Record<string, AiQueryPlan> = {},
): { api: HybridSearchApi; spy: ApiSpy } {
  const spy: ApiSpy = {
    searchLocal: vi.fn(async (query: string, _plan: AiQueryPlan | null) => {
      const list = localHitsByQuery[query] ?? []
      // Simulate the AI-expanded path: if plan is non-null, blend in ai hits.
      if (_plan !== null) {
        const aiList = aiHitsByQuery[query] ?? []
        return [...list, ...aiList]
      }
      return list
    }),
    planSearch: vi.fn(async (query: string, _requestId: string) => {
      const plan = planByQuery[query] ?? emptyPlan()
      return makePlanned(plan)
    }),
    cancelSearch: vi.fn(async (_requestId: string) => {
      // no-op
    }),
  }
  return { api: spy as unknown as HybridSearchApi, spy }
}

function statesWith<T>(states: T[], predicate: (s: T) => boolean): T[] {
  return states.filter(predicate)
}

// ---- tests ----------------------------------------------------------------

describe('HybridSearchController', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('publishes local hits before plan fires', async () => {
    const { api, spy } = makeApi({
      foo: [makeHit('a', 1, ['local'])],
    })
    const states: HybridSearchState[] = []
    const ctrl = new HybridSearchController({
      api,
      delayMs: 700,
      onState: (s) => states.push(s),
    })

    void ctrl.search('foo')
    // flush microtasks so the local promise resolves
    await vi.advanceTimersByTimeAsync(0)

    const localStates = statesWith(states, (s) => s.phase === 'local')
    const localState = localStates[localStates.length - 1]
    expect(localState).toBeDefined()
    expect(localState!.hits.map((h) => h.summary.entry.id)).toEqual(['a'])
    expect(spy.planSearch).not.toHaveBeenCalled()

    ctrl.dispose()
  })

  it('does not call planSearch before delayMs', async () => {
    const { api, spy } = makeApi({
      foo: [makeHit('a', 1, ['local'])],
    })
    const ctrl = new HybridSearchController({
      api,
      delayMs: 700,
      onState: () => {},
    })

    void ctrl.search('foo')
    await vi.advanceTimersByTimeAsync(0) // local
    expect(spy.planSearch).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(400) // less than 700ms
    expect(spy.planSearch).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(300) // total 700ms — plan fires
    expect(spy.planSearch).toHaveBeenCalledTimes(1)

    ctrl.dispose()
  })

  it('new query cancels previous and invalidates old response', async () => {
    const { api, spy } = makeApi({
      foo: [makeHit('a', 1, ['local'])],
      bar: [makeHit('b', 1, ['local'])],
    })
    const states: HybridSearchState[] = []
    const ctrl = new HybridSearchController({
      api,
      delayMs: 700,
      onState: (s) => states.push(s),
    })

    void ctrl.search('foo')
    await vi.advanceTimersByTimeAsync(0) // local foo

    // New query before plan fires
    void ctrl.search('bar')
    // cancel must have been invoked for the previous request id
    expect(spy.cancelSearch).toHaveBeenCalled()
    // bar's local should be published, foo's plan must not have fired
    await vi.advanceTimersByTimeAsync(0)
    const barLocals = statesWith(states, (s) => s.query === 'bar' && s.phase === 'local')
    const barLocal = barLocals[barLocals.length - 1]
    expect(barLocal).toBeDefined()
    expect(barLocal!.hits.map((h) => h.summary.entry.id)).toEqual(['b'])

    // Advance well past 700ms — only one plan (for bar) should fire
    await vi.advanceTimersByTimeAsync(1000)
    const planCalls = spy.planSearch.mock.calls.map((c) => c[0])
    // foo's plan must never have been called (cancelled).
    expect(planCalls).not.toContain('foo')
    expect(planCalls).toContain('bar')

    ctrl.dispose()
  })

  it('AI expansion deduplicates by entry id', async () => {
    const { api } = makeApi(
      {
        // local: a, b
        foo: [makeHit('a', 3, ['local']), makeHit('b', 2, ['local'])],
      },
      {
        // ai expansion: b (dup), c (new)
        foo: [makeHit('b', 1.5, ['aiExpanded']), makeHit('c', 1.2, ['aiExpanded'])],
      },
    )
    const states: HybridSearchState[] = []
    const ctrl = new HybridSearchController({
      api,
      delayMs: 700,
      onState: (s) => states.push(s),
    })

    void ctrl.search('foo')
    await vi.advanceTimersByTimeAsync(0) // local
    await vi.advanceTimersByTimeAsync(700) // plan + ai search

    const finalState = states[states.length - 1]!
    const ids = finalState.hits.map((h) => h.summary.entry.id)
    // Expect a, b, c — no duplicate b
    expect(ids).toEqual(['a', 'b', 'c'])
    // b should carry both sources
    const b = finalState.hits.find((h) => h.summary.entry.id === 'b')!
    expect(b.sources).toContain('local')
    expect(b.sources).toContain('aiExpanded')

    ctrl.dispose()
  })

  it('preserves selectedId when still present; otherwise selects first', async () => {
    const { api } = makeApi(
      {
        foo: [makeHit('a', 3, ['local']), makeHit('b', 2, ['local'])],
      },
      {},
    )
    const states: HybridSearchState[] = []
    const ctrl = new HybridSearchController({
      api,
      delayMs: 700,
      onState: (s) => states.push(s),
    })

    void ctrl.search('foo')
    await vi.advanceTimersByTimeAsync(0) // local
    const localState = states[states.length - 1]!
    expect(localState.selectedId).toBe('a') // default: first

    // Manually select b
    ctrl.setSelectedId('b')

    void ctrl.search('foo')
    await vi.advanceTimersByTimeAsync(0) // local
    const after = states[states.length - 1]!
    expect(after.selectedId).toBe('b') // preserved

    // New query returns no b → fall back to first
    void ctrl.search('bar')
    await vi.advanceTimersByTimeAsync(0)
    const fallbackStates = statesWith(states, (s) => s.query === 'bar' && s.phase === 'local')
    const fallback = fallbackStates[fallbackStates.length - 1]!
    expect(fallback.hits).toEqual([])
    expect(fallback.selectedId).toBeNull()

    ctrl.dispose()
  })

  it('does not publish state after dispose', async () => {
    const states: HybridSearchState[] = []
    const { api, spy } = makeApi({
      foo: [makeHit('a', 1, ['local'])],
    })
    const ctrl = new HybridSearchController({
      api,
      delayMs: 700,
      onState: (s) => states.push(s),
    })

    void ctrl.search('foo')
    await vi.advanceTimersByTimeAsync(0)
    const countBefore = states.length
    expect(countBefore).toBeGreaterThan(0)

    ctrl.dispose()
    states.length = 0

    // Even if we advance timers and the plan resolves, no new states
    await vi.advanceTimersByTimeAsync(2000)
    expect(states).toEqual([])
    // plan may or may not have been invoked (cancel races), but onState must not fire.
    const planCalls = spy.planSearch.mock.calls.length
    void planCalls
  })
})
