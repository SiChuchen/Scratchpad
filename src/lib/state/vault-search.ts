// src/lib/state/vault-search.ts
//
// HybridSearchController —— Vault 混合检索的前端状态协调器。
//
// 行为契约（与 Task 9 IPC 对齐）：
//   * 每次 `search(query)`：
//     1. 立即用 `cancelSearch(previousRequestId)` 取消旧请求（若有）；
//     2. 立即调用 `searchLocal(query, null, limit)`，发布 `phase: 'local'` 状态；
//     3. 设置 `delayMs`（默认 700ms）后调用 `planSearch(query, requestId)`；
//     4. plan 成功后调用 `searchLocal(query, plan, limit)` 得到 AI 扩展命中；
//     5. 把扩展命中与本地命中按 entry id 合并去重，sources 取并集，
//        发布 `phase: 'expanded'` 状态。
//   * `selectedId` 在新结果中存在则保留；否则回退到首项命中（空结果时为 null）。
//   * `dispose()` 后即便旧请求回来也不再发布状态。
//
// requestId 用 `crypto.randomUUID()` 生成；每次 `search()` 都是新的 session。
//
// 该类不持有任何 Svelte store —— 由调用方传入 `onState` 回调驱动 UI；
// 这样便于单元测试（不依赖 Svelte runtime）。

import type {
  AiQueryPlan,
  PlannedSearch,
  SearchSource,
  VaultSearchHit,
} from '$lib/types/vault'

/** Vault search IPC 表面：与 `vaultApi` 子集一致，便于 mock。 */
export interface HybridSearchApi {
  searchLocal(query: string, plan: AiQueryPlan | null, limit?: number): Promise<VaultSearchHit[]>
  planSearch(query: string, requestId: string): Promise<PlannedSearch>
  cancelSearch(requestId: string): Promise<void>
}

export type HybridSearchPhase = 'idle' | 'local' | 'planning' | 'expanded' | 'error'

export interface HybridSearchState {
  query: string
  phase: HybridSearchPhase
  hits: VaultSearchHit[]
  /** AI 查询理解返回的关键词 + 别名（供 UI 预览）。 */
  understoodTerms: string[]
  selectedId: string | null
  /** AI 命中阶段产生的错误（plan 失败 / 网络错误）。null 表示无错误。 */
  error: string | null
}

export interface HybridSearchControllerOptions {
  api: HybridSearchApi
  /** AI plan 触发延迟（默认 700ms）。 */
  delayMs?: number
  /** 每次状态变化时同步调用。 */
  onState: (state: HybridSearchState) => void
  /** 搜索结果上限。 */
  limit?: number
}

const DEFAULT_DELAY_MS = 700
const DEFAULT_LIMIT = 20

/**
 * 混合检索协调器。一个实例只服务一个 UI 容器（如搜索面板）。
 */
export class HybridSearchController {
  private api: HybridSearchApi
  private delayMs: number
  private limit: number
  private onState: (state: HybridSearchState) => void

  private currentQuery = ''
  private currentRequestId: string | null = null
  private currentSelectedId: string | null = null
  private planTimer: ReturnType<typeof setTimeout> | null = null
  private disposed = false

  constructor(opts: HybridSearchControllerOptions) {
    this.api = opts.api
    this.delayMs = opts.delayMs ?? DEFAULT_DELAY_MS
    this.limit = opts.limit ?? DEFAULT_LIMIT
    this.onState = opts.onState
  }

  /**
   * 触发一次新搜索。会取消任何进行中的旧搜索。
   *
   * 调用后：
   *   - 同步发布 `phase: 'local'` 占位状态（query 更新、hits 清空）；
   *   - 微任务结束后发布本地结果；
   *   - delayMs 后触发 planSearch + 扩展检索。
   */
  search(query: string): Promise<void> {
    if (this.disposed) return Promise.resolve()

    // 1) Cancel any in-flight search for the previous request id.
    const previousRequestId = this.currentRequestId
    if (previousRequestId !== null) {
      // Fire and forget: cancel is best-effort.
      void this.api.cancelSearch(previousRequestId).catch(() => {})
    }
    if (this.planTimer !== null) {
      clearTimeout(this.planTimer)
      this.planTimer = null
    }

    // 2) Start a new session.
    const requestId = crypto.randomUUID()
    this.currentQuery = query
    this.currentRequestId = requestId

    // 3) Publish placeholder state.
    this.publish({
      query,
      phase: 'local',
      hits: [],
      understoodTerms: [],
      selectedId: this.currentSelectedId,
      error: null,
    })

    // 4) Local search (no plan) immediately.
    const localPromise = this.api
      .searchLocal(query, null, this.limit)
      .then((hits) => {
        if (this.currentRequestId !== requestId || this.disposed) return
        this.currentSelectedId = pickSelected(hits, this.currentSelectedId)
        this.publish({
          query,
          phase: 'local',
          hits,
          understoodTerms: [],
          selectedId: this.currentSelectedId,
          error: null,
        })
      })
      .catch((err) => {
        if (this.currentRequestId !== requestId || this.disposed) return
        this.publish({
          query,
          phase: 'error',
          hits: [],
          understoodTerms: [],
          selectedId: null,
          error: errorMessage(err),
        })
      })

    // 5) After delayMs, trigger plan + AI expansion.
    this.planTimer = setTimeout(() => {
      this.planTimer = null
      if (this.currentRequestId !== requestId || this.disposed) return
      this.publish({
        query,
        phase: 'planning',
        hits: [],
        understoodTerms: [],
        selectedId: this.currentSelectedId,
        error: null,
      })
      // Wait for local to finish first so we can blend properly.
      void localPromise.finally(() => {
        if (this.currentRequestId !== requestId || this.disposed) return
        void this.runPlanAndExpand(query, requestId)
      })
    }, this.delayMs)

    return Promise.resolve()
  }

  /** 显式覆盖当前选中的 entry id（用户点击结果时调用）。 */
  setSelectedId(id: string | null): void {
    this.currentSelectedId = id
  }

  /** 释放资源；之后任何 in-flight 回调都不会再触发 onState。 */
  dispose(): void {
    this.disposed = true
    if (this.planTimer !== null) {
      clearTimeout(this.planTimer)
      this.planTimer = null
    }
    if (this.currentRequestId !== null) {
      void this.api.cancelSearch(this.currentRequestId).catch(() => {})
    }
    this.currentRequestId = null
  }

  // ---- internal ----------------------------------------------------------

  private async runPlanAndExpand(query: string, requestId: string): Promise<void> {
    let planned: PlannedSearch
    try {
      planned = await this.api.planSearch(query, requestId)
    } catch (err) {
      if (this.currentRequestId !== requestId || this.disposed) return
      this.publish({
        query,
        phase: 'error',
        hits: [],
        understoodTerms: [],
        selectedId: this.currentSelectedId,
        error: errorMessage(err),
      })
      return
    }
    if (this.currentRequestId !== requestId || this.disposed) return

    let expanded: VaultSearchHit[]
    try {
      expanded = await this.api.searchLocal(query, planned.plan, this.limit)
    } catch (err) {
      if (this.currentRequestId !== requestId || this.disposed) return
      this.publish({
        query,
        phase: 'error',
        hits: [],
        understoodTerms: planned.understoodTerms,
        selectedId: this.currentSelectedId,
        error: errorMessage(err),
      })
      return
    }
    if (this.currentRequestId !== requestId || this.disposed) return

    // Merge local + expanded by entry id.
    const merged = mergeByEntryId(expanded)
    this.currentSelectedId = pickSelected(merged, this.currentSelectedId)
    this.publish({
      query,
      phase: 'expanded',
      hits: merged,
      understoodTerms: planned.understoodTerms,
      selectedId: this.currentSelectedId,
      error: null,
    })
  }

  private publish(state: HybridSearchState): void {
    if (this.disposed) return
    this.onState(state)
  }
}

// ---- helpers --------------------------------------------------------------

/**
 * Pick the selected id from a new list of hits. If the previously selected id
 * still exists, keep it; otherwise fall back to the first hit (or null when
 * empty).
 */
function pickSelected(hits: VaultSearchHit[], previous: string | null): string | null {
  if (hits.length === 0) return null
  const ids = hits.map((h) => h.summary.entry.id)
  if (previous !== null && ids.includes(previous)) return previous
  return ids[0]
}

/**
 * Merge hits by entry id. The output order is stable: later hits for the same
 * id are dropped; sources are unioned; score takes the max.
 *
 * Input may include the same entry id multiple times when local + AI expansion
 * both surface it. We merge into a single hit per id, preserving the first
 * occurrence's position so the user sees a stable list.
 */
function mergeByEntryId(hits: VaultSearchHit[]): VaultSearchHit[] {
  const out: VaultSearchHit[] = []
  const index = new Map<string, number>()
  for (const h of hits) {
    const id = h.summary.entry.id
    const existingPos = index.get(id)
    if (existingPos === undefined) {
      index.set(id, out.length)
      out.push({
        summary: h.summary,
        score: h.score,
        sources: [...h.sources],
      })
    } else {
      const existing = out[existingPos]
      const mergedSources = new Set<SearchSource>(existing.sources)
      for (const s of h.sources) mergedSources.add(s)
      existing.sources = [...mergedSources]
      if (h.score > existing.score) existing.score = h.score
    }
  }
  return out
}

/** Generate a v4 UUID via Web Crypto (available in browser & vitest jsdom). */
// (Helper kept inline to avoid extra imports; crypto.randomUUID is standard.)
function errorMessage(err: unknown): string {
  if (typeof err === 'string') return err
  if (err instanceof Error) return err.message
  return String(err)
}
