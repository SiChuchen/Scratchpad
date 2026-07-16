// src/lib/state/library-view.test.ts
//
// LibraryViewController 行为测试 —— Task 13 + 修复 C1/C2/I2。
//
// 覆盖场景：
//   1. countsFrom / filterEntries 等纯函数 helper（调用方 $state 列表
//      驱动，过滤 pending-delete）。
//   2. 删除先把 id 标记为 pending；3 秒后才调用后端。
//   3. 撤销点击通过 onRestoreUndo 恢复条目且不调用后端删除。
//   4. 后端删除失败时通过 onRestoreFailedDelete 把 pending.summary
//      交回调用方，并 notify 错误。
//   5. C1: 模拟 Svelte $state 模式 —— 调用方拥有 entries 数组，
//      filterEntries 正确排除 pending-delete。
//   6. C2: undoDelete 在 commitDelete 进行中被 race-guard 拒绝，
//      不会与 commit-fail 的 onRestoreFailedDelete 重复恢复条目。
//   7. dispose 取消 pending delete timers。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  LibraryViewController,
  type DeletePendingEntry,
  type LibraryFilter,
  type LibraryNotify,
} from './library-view'
import type { EntryKind, VaultEntrySummary } from '$lib/types/vault'

// ---- helpers --------------------------------------------------------------

function makeSummary(id: string, kind: EntryKind, title?: string): VaultEntrySummary {
  return {
    entry: {
      id,
      kind,
      title: title ?? `${kind}-${id}`,
      notes: null,
      createdAt: '2026-07-17T00:00:00Z',
      updatedAt: '2026-07-17T00:00:00Z',
    },
    tags: [],
    preview: null,
  }
}

function makeEntries(): VaultEntrySummary[] {
  return [
    makeSummary('a', 'credential'),
    makeSummary('b', 'bookmark'),
    makeSummary('c', 'note'),
    makeSummary('d', 'credential'),
    makeSummary('e', 'note'),
  ]
}

interface DeleteApi {
  onDelete: ReturnType<typeof vi.fn>
}

interface NotifyCalls {
  notify: ReturnType<typeof vi.fn>
}

interface RestoreSpies {
  onRestoreFailedDelete: ReturnType<typeof vi.fn>
  onRestoreUndo: ReturnType<typeof vi.fn>
}

function makeController(opts?: {
  entries?: VaultEntrySummary[]
  deleteDelayMs?: number
  deleteResult?: 'reject' | 'resolve'
  /** 控制 onDelete 何时 resolve；用于 race 测试。 */
  deleteBlock?: () => Promise<void>
}): {
  ctrl: LibraryViewController
  delSpy: DeleteApi
  notifySpy: NotifyCalls
  restoreSpies: RestoreSpies
} {
  const delSpy: DeleteApi = {
    onDelete: vi.fn(async (_id: string) => {
      if (opts?.deleteBlock) await opts.deleteBlock()
      if (opts?.deleteResult === 'reject') throw new Error('backend-failed')
    }),
  }
  const notifySpy: NotifyCalls = {
    notify: vi.fn(),
  }
  const restoreSpies: RestoreSpies = {
    onRestoreFailedDelete: vi.fn(),
    onRestoreUndo: vi.fn(),
  }
  const ctrl = new LibraryViewController({
    onDelete: delSpy.onDelete as (id: string) => Promise<void>,
    notify: notifySpy.notify as LibraryNotify,
    onRestoreFailedDelete: restoreSpies.onRestoreFailedDelete as (p: DeletePendingEntry) => void,
    onRestoreUndo: restoreSpies.onRestoreUndo as (p: DeletePendingEntry) => void,
    deleteDelayMs: opts?.deleteDelayMs ?? 3000,
  })
  if (opts?.entries !== undefined) {
    ctrl.setAllEntries(opts.entries)
  } else {
    ctrl.setAllEntries(makeEntries())
  }
  return { ctrl, delSpy, notifySpy, restoreSpies }
}

// ---- tests ----------------------------------------------------------------

describe('LibraryViewController', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('countsFrom counts all/credential/bookmark/note from a caller-supplied list', () => {
    const { ctrl } = makeController()
    const counts = ctrl.countsFrom(makeEntries())
    expect(counts).toEqual({ all: 5, credential: 2, bookmark: 1, note: 2 })
  })

  it('filterEntries filters by kind when filter is not "all"', () => {
    const { ctrl } = makeController()
    const entries = makeEntries()
    const allFiltered = ctrl.filterEntries(entries, 'all' as LibraryFilter)
    expect(allFiltered.length).toBe(5)
    const credFiltered = ctrl.filterEntries(entries, 'credential' as LibraryFilter)
    expect(credFiltered.length).toBe(2)
    expect(credFiltered.every((e) => e.entry.kind === 'credential')).toBe(true)
  })

  it('requestDelete marks id as pending and commits to backend after delay', async () => {
    const { ctrl, delSpy } = makeController()
    expect(ctrl.isPendingDelete('a')).toBe(false)

    ctrl.requestDelete('a')
    // Immediately pending
    expect(ctrl.isPendingDelete('a')).toBe(true)
    expect(delSpy.onDelete).not.toHaveBeenCalled()

    // Backend not called yet
    await vi.advanceTimersByTimeAsync(2999)
    expect(delSpy.onDelete).not.toHaveBeenCalled()

    // After 3 seconds
    await vi.advanceTimersByTimeAsync(1)
    expect(delSpy.onDelete).toHaveBeenCalledTimes(1)
    expect(delSpy.onDelete.mock.calls[0]![0]).toBe('a')
    // No longer pending after commit success
    expect(ctrl.isPendingDelete('a')).toBe(false)
  })

  it('undoDelete restores via onRestoreUndo and skips backend', () => {
    const { ctrl, delSpy, restoreSpies, notifySpy } = makeController()
    expect(ctrl.isPendingDelete('a')).toBe(false)

    ctrl.requestDelete('a')
    expect(ctrl.isPendingDelete('a')).toBe(true)
    expect(notifySpy.notify).toHaveBeenCalledTimes(1)
    const notifyArgs = notifySpy.notify.mock.calls[0]!
    const undoFn = notifyArgs[2] as (() => void) | undefined
    expect(typeof undoFn).toBe('function')

    undoFn!()

    // No longer pending
    expect(ctrl.isPendingDelete('a')).toBe(false)
    // onRestoreUndo invoked once
    expect(restoreSpies.onRestoreUndo).toHaveBeenCalledTimes(1)
    const pending = restoreSpies.onRestoreUndo.mock.calls[0]![0] as DeletePendingEntry
    expect(pending.id).toBe('a')
    expect(pending.summary.entry.id).toBe('a')
    // Backend NOT called
    expect(delSpy.onDelete).not.toHaveBeenCalled()
    // Flushing timers also does not call backend
    vi.runAllTimers()
    expect(delSpy.onDelete).not.toHaveBeenCalled()
  })

  it('backend delete failure invokes onRestoreFailedDelete and notifies error', async () => {
    const { ctrl, delSpy, notifySpy, restoreSpies } = makeController({ deleteResult: 'reject' })

    ctrl.requestDelete('a')
    await vi.advanceTimersByTimeAsync(3000)

    expect(delSpy.onDelete).toHaveBeenCalledTimes(1)
    expect(restoreSpies.onRestoreFailedDelete).toHaveBeenCalledTimes(1)
    const pending = restoreSpies.onRestoreFailedDelete.mock.calls[0]![0] as DeletePendingEntry
    expect(pending.id).toBe('a')
    const errorCall = notifySpy.notify.mock.calls.find((c) => c[1] === 'error')
    expect(errorCall).toBeDefined()
  })

  it('dispose cancels pending delete timers', async () => {
    const { ctrl, delSpy } = makeController({ deleteDelayMs: 3000 })
    ctrl.requestDelete('a')
    ctrl.dispose()
    await vi.advanceTimersByTimeAsync(5000)
    expect(delSpy.onDelete).not.toHaveBeenCalled()
  })

  // --- C1 regression: pending-delete filtering via $state-driven helper ---

  it('C1: clicking delete immediately removes entry from filterEntries output', () => {
    // Simulate VaultView: caller owns entries array, derived reads via
    // filterEntries + isPendingDelete. After requestDelete, the entry
    // should NOT appear in filterEntries output (without modifying the
    // caller's array).
    const { ctrl } = makeController()
    const entries = makeEntries()
    // Initial: 'a' visible
    expect(ctrl.filterEntries(entries, 'all').find((e) => e.entry.id === 'a')).toBeDefined()

    ctrl.requestDelete('a')

    // After delete: 'a' filtered out, but entries array untouched.
    const visibleAfter = ctrl.filterEntries(entries, 'all')
    expect(visibleAfter.find((e) => e.entry.id === 'a')).toBeUndefined()
    expect(visibleAfter.length).toBe(4)
    // Underlying array not mutated
    expect(entries.length).toBe(5)

    // countsFrom reflects the filtered set
    const counts = ctrl.countsFrom(entries)
    expect(counts.all).toBe(4)
    expect(counts.credential).toBe(1) // 'd' only; 'a' excluded
  })

  // --- C2 regression: undo during commit does not duplicate ---

  it('C2: undoDelete during commitDelete is refused (no duplicate restore)', async () => {
    // Setup: onDelete blocks on a deferred we control.
    let resolveDelete: () => void = () => {}
    const deleteBlock = new Promise<void>((resolve) => {
      resolveDelete = resolve
    })
    const { ctrl, delSpy, restoreSpies } = makeController({
      deleteResult: 'reject',
      deleteBlock: () => deleteBlock,
    })

    ctrl.requestDelete('a')
    expect(ctrl.isPendingDelete('a')).toBe(true)

    // Fire the commit timer; this enters commitDelete which awaits onDelete.
    await vi.advanceTimersByTimeAsync(3000)
    // Now commit is in flight (onDelete pending). undo should be refused.
    expect(ctrl.isCommitting('a')).toBe(true)
    const undoResult = ctrl.undoDelete('a')
    expect(undoResult).toBeNull()
    expect(restoreSpies.onRestoreUndo).not.toHaveBeenCalled()

    // Now let onDelete reject. commit-fail path should fire onRestoreFailedDelete
    // exactly once (not twice from a duplicate undo).
    resolveDelete()
    // Allow microtasks to flush
    await Promise.resolve()
    await Promise.resolve()

    expect(delSpy.onDelete).toHaveBeenCalledTimes(1)
    expect(restoreSpies.onRestoreFailedDelete).toHaveBeenCalledTimes(1)
    // Undo never restored
    expect(restoreSpies.onRestoreUndo).not.toHaveBeenCalled()
    // No longer committing
    expect(ctrl.isCommitting('a')).toBe(false)
  })
})
