// src/lib/state/library-view.ts
//
// LibraryViewController —— 主窗口"资料库"视图的前端状态协调器。
//
// 职责：
//   * 持有 all-entries 单一数据源，并在前端按 kind 过滤；
//   * 维护 searchQuery / searchStarted / searchHits 三态（区分"未搜索"
//     与"空结果"）；
//   * 提供 counts（all/credential/bookmark/note 来自同一列表）；
//   * 删除采用乐观 UI + 3 秒延迟提交后端 + 撤销；失败时恢复条目并
//     通知错误。
//
// 该类不持有 Svelte store —— 由调用方传入 `notify` 回调驱动外部 UI；
// 这样便于单元测试（不依赖 Svelte runtime）。

import type {
  EntryKind,
  VaultEntrySummary,
  VaultSearchHit,
} from '$lib/types/vault'

/** 库筛选值。 */
export type LibraryFilter = 'all' | EntryKind

/** 各 kind 计数（来自 all-entries 列表）。 */
export interface LibraryCounts {
  all: number
  credential: number
  bookmark: number
  note: number
}

/**
 * 一条待提交的删除记录。从 UI 移除后，等待 `deleteDelayMs` 才调用后端；
 * 在该窗口内可被 `undoDelete(id)` 恢复。
 */
export interface DeletePendingEntry {
  id: string
  summary: VaultEntrySummary
  originalIndex: number
  timer: ReturnType<typeof setTimeout>
}

/** 外部通知签名 —— 与 App.showToast 兼容。 */
export type LibraryNotify = (
  text: string,
  kind?: 'success' | 'error',
  undo?: () => void,
  actionLabel?: string,
) => void

export interface LibraryViewControllerOptions {
  /**
   * 实际提交删除的后端调用。失败时由 controller 恢复条目并通知错误。
   */
  onDelete: (id: string) => Promise<void>
  /**
   * 用户可见通知回调（撤销 toast、错误 toast 等）。
   */
  notify: LibraryNotify
  /** 删除延迟，默认 3000ms。 */
  deleteDelayMs?: number
}

/** Library view 快照 —— 给调用方渲染 UI 使用。 */
export interface LibraryState {
  allEntries: VaultEntrySummary[]
  filter: LibraryFilter
  searchQuery: string
  /** 用户是否已提交过搜索（用于区分"未搜索"与"空结果"）。 */
  searchStarted: boolean
  /** 搜索命中；null 表示当前未处于搜索态。 */
  searchHits: VaultSearchHit[] | null
  pendingDeletes: Map<string, DeletePendingEntry>
}

const DEFAULT_DELETE_DELAY_MS = 3000

/**
 * 资料库视图协调器。一个实例只服务一个 VaultView。
 */
export class LibraryViewController {
  private allEntries: VaultEntrySummary[] = []
  private filter: LibraryFilter = 'all'
  private searchQuery = ''
  private searchStarted = false
  private searchHits: VaultSearchHit[] | null = null
  private pendingDeletes = new Map<string, DeletePendingEntry>()
  private onDelete: (id: string) => Promise<void>
  private notify: LibraryNotify
  private deleteDelayMs: number
  private disposed = false

  constructor(opts: LibraryViewControllerOptions) {
    this.onDelete = opts.onDelete
    this.notify = opts.notify
    this.deleteDelayMs = opts.deleteDelayMs ?? DEFAULT_DELETE_DELAY_MS
  }

  // ---- setters ------------------------------------------------------------

  /** 设置 all-entries 数据源（用于 listEntries / 事件刷新）。 */
  setAllEntries(entries: VaultEntrySummary[]): void {
    this.allEntries = entries
  }

  setFilter(filter: LibraryFilter): void {
    this.filter = filter
  }

  setSearchQuery(query: string): void {
    this.searchQuery = query
  }

  /** 标记用户已开始搜索（首次提交非空 query 时为 true）。 */
  setSearchStarted(started: boolean): void {
    this.searchStarted = started
  }

  /** 设置当前搜索命中。null 表示退出搜索态（如清空）。 */
  setSearchHits(hits: VaultSearchHit[] | null): void {
    this.searchHits = hits
  }

  // ---- queries ------------------------------------------------------------

  /** 当前快照（不可变副本）。 */
  getState(): LibraryState {
    return {
      allEntries: this.allEntries.slice(),
      filter: this.filter,
      searchQuery: this.searchQuery,
      searchStarted: this.searchStarted,
      searchHits: this.searchHits ? this.searchHits.slice() : null,
      pendingDeletes: new Map(this.pendingDeletes),
    }
  }

  /**
   * all/credential/bookmark/note 计数，全部来自同一 all-entries 列表
   * （已经乐观移除的待删除项不参与计数）。
   */
  counts(): LibraryCounts {
    const visible = this.visibleEntries()
    let credential = 0
    let bookmark = 0
    let note = 0
    for (const e of visible) {
      if (e.entry.kind === 'credential') credential++
      else if (e.entry.kind === 'bookmark') bookmark++
      else if (e.entry.kind === 'note') note++
    }
    return {
      all: visible.length,
      credential,
      bookmark,
      note,
    }
  }

  /**
   * 返回当前应渲染的条目列表：
   *   * 搜索态（searchHits 非 null）→ 返回 hits 中 summary 的数组（前端
   *     不再按 filter 过滤，因为 hits 已经是相关性结果）；
   *   * 非搜索态 → 按 filter 过滤后的 all-entries。
   *
   * 不返回 searchHits 本身是为了让渲染层始终拿到 VaultEntrySummary[]；
   * 调用方需要 score 时另行读取 hits。
   */
  filtered(): VaultEntrySummary[] {
    const visible = this.visibleEntries()
    if (this.filter === 'all') return visible
    return visible.filter((e) => e.entry.kind === this.filter)
  }

  /** 是否处于"已开始搜索"且 hits 非 null 的状态。 */
  isSearching(): boolean {
    return this.searchStarted && this.searchHits !== null
  }

  // ---- delete / undo ------------------------------------------------------

  /**
   * 乐观删除：立即从 allEntries 中移除条目，并在 `deleteDelayMs` 后调用
   * 后端；同时通过 notify 触发"撤销"toast。
   *
   * 在延迟窗口内调用 undoDelete(id) 可取消提交并恢复条目。
   */
  requestDelete(id: string): void {
    if (this.disposed) return
    if (this.pendingDeletes.has(id)) return
    const idx = this.allEntries.findIndex((e) => e.entry.id === id)
    if (idx === -1) return
    const summary = this.allEntries[idx]!
    // 立即从 UI 移除
    this.allEntries = this.allEntries.filter((e) => e.entry.id !== id)
    const timer = setTimeout(() => {
      void this.commitDelete(id)
    }, this.deleteDelayMs)
    const pending: DeletePendingEntry = { id, summary, originalIndex: idx, timer }
    this.pendingDeletes.set(id, pending)

    const undo = () => this.undoDelete(id)
    this.notify('已删除', 'success', undo, '撤销')
  }

  /**
   * 撤销删除：取消 pending timer，恢复条目到原位置，不调用后端。
   */
  undoDelete(id: string): void {
    const pending = this.pendingDeletes.get(id)
    if (!pending) return
    clearTimeout(pending.timer)
    this.pendingDeletes.delete(id)
    // 恢复到 originalIndex（如果越界则插到末尾）
    const insertAt = Math.min(pending.originalIndex, this.allEntries.length)
    const next = this.allEntries.slice()
    next.splice(insertAt, 0, pending.summary)
    this.allEntries = next
  }

  /**
   * 释放资源；取消所有 pending timers。dispose 后即便 timer 已经触发，
   * 也跳过回调（disposed 标志）。
   */
  dispose(): void {
    this.disposed = true
    for (const pending of this.pendingDeletes.values()) {
      clearTimeout(pending.timer)
    }
    this.pendingDeletes.clear()
  }

  // ---- internal -----------------------------------------------------------

  /**
   * 真正提交删除；失败时恢复条目并 notify 错误。
   */
  private async commitDelete(id: string): Promise<void> {
    if (this.disposed) return
    const pending = this.pendingDeletes.get(id)
    if (!pending) return
    try {
      await this.onDelete(id)
      // 成功：仅清理 pending（条目已在 UI 移除）
      this.pendingDeletes.delete(id)
    } catch (err) {
      if (this.disposed) return
      // 失败：恢复到原位置
      this.pendingDeletes.delete(id)
      const insertAt = Math.min(pending.originalIndex, this.allEntries.length)
      const next = this.allEntries.slice()
      next.splice(insertAt, 0, pending.summary)
      this.allEntries = next
      const msg = err instanceof Error && err.message
        ? err.message
        : typeof err === 'string' ? err : '未知错误'
      this.notify(`删除失败：${msg}`, 'error')
    }
  }

  /**
   * 返回未被乐观删除的条目（allEntries 已经是过滤后的列表，这里只是
   * 一个语义清晰的内部别名）。
   */
  private visibleEntries(): VaultEntrySummary[] {
    return this.allEntries
  }
}
