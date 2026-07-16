// src/lib/state/library-view.ts
//
// LibraryViewController —— 主窗口"资料库"视图的前端状态协调器。
//
// 职责：
//   * 协调删除的乐观 UI + 3 秒延迟提交后端 + 撤销；失败时恢复条目并
//     通知错误。
//   * 暴露纯函数 helper（countsFrom / filterFrom）让调用方在自己的
//     Svelte `$state` 数组上派生 UI（解决 reactivity 桥接问题）。
//   * 维护 pendingDeletes / committingIds 两组 ID 集合，确保：
//       - 渲染层可通过 isPendingDelete(id) 过滤掉待删除项；
//       - undoDelete 在 commit 进行中时拒绝，避免双重恢复导致重复条目。
//
// 该类不持有 Svelte store —— 由调用方传入 `notify` 回调驱动外部 UI；
// 这样便于单元测试（不依赖 Svelte runtime）。all-entries 的真相数据源
// 由调用方（VaultView）以 `$state` 持有，本类只读 snapshot。

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

/**
 * 当 commit 失败需要把条目恢复到调用方 $state 时调用。调用方实现：
 * 把 summary 插入其 allEntries（位置由 originalIndex 决定）。
 *
 * 若调用方仅关心通知而不需要恢复（如测试），可省略该回调。
 */
export type LibraryRestoreEntry = (pending: DeletePendingEntry) => void

export interface LibraryViewControllerOptions {
  /**
   * 实际提交删除的后端调用。失败时由 controller 通过 onRestoreFailedDelete
   * 恢复条目并 notify 错误。
   */
  onDelete: (id: string) => Promise<void>
  /**
   * 用户可见通知回调（撤销 toast、错误 toast 等）。
   */
  notify: LibraryNotify
  /**
   * commit 失败时由 controller 调用；调用方据此把 pending.summary
   * 恢复回 $state 的 allEntries。
   */
  onRestoreFailedDelete?: LibraryRestoreEntry
  /**
   * undoDelete 成功时由 controller 调用；调用方据此把 pending.summary
   * 恢复回 $state 的 allEntries。若未提供，undo 不恢复（仅取消 timer）。
   */
  onRestoreUndo?: LibraryRestoreEntry
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
 *
 * 注意：本类不再持有 `allEntries` 真相数据源 —— 该数据源由调用方
 * （VaultView）以 Svelte `$state` 持有，这样 `$derived` 才能正确追踪
 * 变化。本类只读 snapshot + 维护删除事务。
 */
export class LibraryViewController {
  /** 由调用方维护并随时可被替换的 snapshot；用于 commit/undo 的恢复操作。 */
  private allEntries: VaultEntrySummary[] = []
  private pendingDeletes = new Map<string, DeletePendingEntry>()
  /**
   * 已经进入 commitDelete 的 id；undoDelete 见到则拒绝，防止 race
   * 条件下双重恢复导致条目重复。
   */
  private committingIds = new Set<string>()
  /**
   * 版本号，调用方可在 $derived 中读取以强制重算（即便使用了 plain
   * 字段，外部读 pendingDeletes 的 size 仍可作为依赖触发器）。
   */
  private pendingVersion = 0
  private onDelete: (id: string) => Promise<void>
  private notify: LibraryNotify
  private onRestoreFailedDelete: LibraryRestoreEntry | undefined
  private onRestoreUndo: LibraryRestoreEntry | undefined
  private deleteDelayMs: number
  private disposed = false

  constructor(opts: LibraryViewControllerOptions) {
    this.onDelete = opts.onDelete
    this.notify = opts.notify
    this.onRestoreFailedDelete = opts.onRestoreFailedDelete
    this.onRestoreUndo = opts.onRestoreUndo
    this.deleteDelayMs = opts.deleteDelayMs ?? DEFAULT_DELETE_DELAY_MS
  }

  // ---- setters ------------------------------------------------------------

  /**
   * 同步当前 all-entries snapshot（由调用方的 $state 注入）。
   * 注意：调用方负责过滤掉 `isPendingDelete(id)` 的条目，避免 reload
   * 把乐观删除的条目复活。
   */
  setAllEntries(entries: VaultEntrySummary[]): void {
    this.allEntries = entries
  }

  /** 显式让 controller 知道当前 all-entries（用于 undo 恢复时插入）。 */
  syncAllEntries(entries: VaultEntrySummary[]): void {
    this.allEntries = entries
  }

  // ---- queries ------------------------------------------------------------

  /** 当前快照（不可变副本）。 */
  getState(): LibraryState {
    return {
      allEntries: this.allEntries.slice(),
      filter: 'all',
      searchQuery: '',
      searchStarted: false,
      searchHits: null,
      pendingDeletes: new Map(this.pendingDeletes),
    }
  }

  /**
   * 给定 all-entries 列表，返回过滤掉 pending-delete 后的可见条目。
   * 让调用方在 $state 上派生 UI。
   */
  filterPending(entries: VaultEntrySummary[]): VaultEntrySummary[] {
    return entries.filter((e) => !this.isPendingDelete(e.entry.id))
  }

  /** 给定 all-entries 列表与 filter，返回按 kind 过滤后的可见条目。 */
  filterEntries(entries: VaultEntrySummary[], filter: LibraryFilter): VaultEntrySummary[] {
    const visible = this.filterPending(entries)
    if (filter === 'all') return visible
    return visible.filter((e) => e.entry.kind === filter)
  }

  /** 给定 all-entries 列表，返回 all/credential/bookmark/note 计数（排除 pending）。 */
  countsFrom(entries: VaultEntrySummary[]): LibraryCounts {
    const visible = this.filterPending(entries)
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

  /** 该 id 当前是否在 pending-delete 窗口内（渲染时用于过滤）。 */
  isPendingDelete(id: string): boolean {
    return this.pendingDeletes.has(id)
  }

  /** 该 id 是否正处于 commitDelete 进行中（用于 race guard）。 */
  isCommitting(id: string): boolean {
    return this.committingIds.has(id)
  }

  /**
   * 版本号；调用方可在 $derived 中读取该值以建立对 pendingDeletes 变化的
   * 依赖（即便不直接读 Map）。
   */
  pendingDeleteVersion(): number {
    return this.pendingVersion
  }

  /**
   * 返回当前 pending-delete 的 id 集合（不可变副本）。调用方可在
   * Svelte `$state` 中存储以便 derived 追踪。
   */
  pendingDeleteIds(): string[] {
    return Array.from(this.pendingDeletes.keys())
  }

  // ---- delete / undo ------------------------------------------------------

  /**
   * 乐观删除：立即把 id 加入 pendingDeletes（渲染层据此过滤），并在
   * `deleteDelayMs` 后调用后端；同时通过 notify 触发"撤销"toast。
   *
   * 在延迟窗口内调用 undoDelete(id) 可取消提交。
   *
   * 调用方负责从其 `$state` 的 allEntries 中过滤掉 pending id 以驱动
   * UI 重渲染；本方法不直接修改 allEntries。
   */
  requestDelete(id: string): void {
    if (this.disposed) return
    if (this.pendingDeletes.has(id)) return
    if (this.committingIds.has(id)) return
    const idx = this.allEntries.findIndex((e) => e.entry.id === id)
    if (idx === -1) return
    const summary = this.allEntries[idx]!
    const timer = setTimeout(() => {
      void this.commitDelete(id)
    }, this.deleteDelayMs)
    const pending: DeletePendingEntry = { id, summary, originalIndex: idx, timer }
    this.pendingDeletes.set(id, pending)
    this.pendingVersion++

    const undo = () => this.undoDelete(id)
    this.notify('已删除', 'success', undo, '撤销')
  }

  /**
   * 撤销删除：取消 pending timer，从 pendingDeletes 移除 id；通过
   * `onRestoreUndo` 回调让调用方把条目恢复到其 $state。本方法不直接
   * 修改 allEntries —— 恢复语义由调用方实现（它持有 $state）。
   *
   * 返回被撤销的 pending 信息（含 originalIndex）；若 id 已不在 pending
   * 或正在 commit（race guard），返回 null。
   */
  undoDelete(id: string): DeletePendingEntry | null {
    // Race guard: 若 commit 已经在飞行中，拒绝 undo（避免 commit 失败
    // 恢复 + undo 恢复 = 重复）。
    if (this.committingIds.has(id)) return null
    const pending = this.pendingDeletes.get(id)
    if (!pending) return null
    clearTimeout(pending.timer)
    this.pendingDeletes.delete(id)
    this.pendingVersion++
    if (this.onRestoreUndo) {
      this.onRestoreUndo(pending)
    }
    return pending
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
    this.committingIds.clear()
  }

  // ---- internal -----------------------------------------------------------

  /**
   * 真正提交删除；失败时通知调用方恢复条目。
   *
   * Race-safe 实现：
   *   1. 同步把 id 从 pendingDeletes 移到 committingIds（在 await 之前），
   *      这样 undoDelete 在 await 期间被调用会被 race guard 拒绝。
   *   2. await onDelete(id)。
   *   3. 成功：从 committingIds 移除即可。
   *   4. 失败：通过 onRestoreFailedDelete 回调把 pending.summary 交回调用方
   *      插入 $state；同时 notify 错误 toast。
   */
  private async commitDelete(id: string): Promise<void> {
    if (this.disposed) return
    const pending = this.pendingDeletes.get(id)
    if (!pending) return

    // 同步：从 pending 移到 committing，避免 undo 在 await 期间命中。
    this.pendingDeletes.delete(id)
    this.pendingVersion++
    this.committingIds.add(id)

    try {
      await this.onDelete(id)
      if (this.disposed) return
      // 成功：条目早已不在调用方 $state（pending-delete 过滤掉了）。
    } catch (err) {
      if (this.disposed) return
      // 失败：通知错误 + 把恢复责任交给调用方。
      const msg = err instanceof Error && err.message
        ? err.message
        : typeof err === 'string' ? err : '未知错误'
      if (this.onRestoreFailedDelete) {
        this.onRestoreFailedDelete(pending)
      }
      this.notify(`删除失败：${msg}`, 'error')
    } finally {
      this.committingIds.delete(id)
    }
  }
}
