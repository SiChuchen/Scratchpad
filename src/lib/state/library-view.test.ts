// src/lib/state/library-view.test.ts
//
// LibraryViewController 行为测试 —— Task 13 Step 1。
//
// 覆盖六个必测场景：
//   1. all / credential / bookmark / note 计数来自同一 all entries 列表。
//   2. filter 切换不会清除 searchQuery。
//   3. 空搜索结果 vs 未开始搜索是不同状态。
//   4. 删除先从 UI 移除，3 秒后才调用后端。
//   5. 撤销点击恢复原位置且不调用后端删除。
//   6. 后端删除失败时恢复条目并通知错误。

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  LibraryViewController,
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

function makeController(opts?: {
  entries?: VaultEntrySummary[]
  deleteDelayMs?: number
  deleteResult?: 'reject' | 'resolve'
}): { ctrl: LibraryViewController; delSpy: DeleteApi; notifySpy: NotifyCalls } {
  const delSpy: DeleteApi = {
    onDelete: vi.fn(async (_id: string) => {
      if (opts?.deleteResult === 'reject') throw new Error('backend-failed')
    }),
  }
  const notifySpy: NotifyCalls = {
    notify: vi.fn(),
  }
  const ctrl = new LibraryViewController({
    onDelete: delSpy.onDelete as (id: string) => Promise<void>,
    notify: notifySpy.notify as LibraryNotify,
    deleteDelayMs: opts?.deleteDelayMs ?? 3000,
  })
  if (opts?.entries !== undefined) {
    ctrl.setAllEntries(opts.entries)
  } else {
    ctrl.setAllEntries(makeEntries())
  }
  return { ctrl, delSpy, notifySpy }
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

  it('counts all/credential/bookmark/note from the same all-entries list', () => {
    const { ctrl } = makeController()
    const counts = ctrl.counts()
    expect(counts).toEqual({ all: 5, credential: 2, bookmark: 1, note: 2 })
  })

  it('switching filter does not clear searchQuery', () => {
    const { ctrl } = makeController()
    ctrl.setSearchQuery('foo')
    expect(ctrl.getState().searchQuery).toBe('foo')

    ctrl.setFilter('credential' as LibraryFilter)
    expect(ctrl.getState().searchQuery).toBe('foo')

    ctrl.setFilter('note' as LibraryFilter)
    expect(ctrl.getState().searchQuery).toBe('foo')

    ctrl.setFilter('all' as LibraryFilter)
    expect(ctrl.getState().searchQuery).toBe('foo')
  })

  it('distinguishes empty search results from not-started-search', () => {
    const { ctrl } = makeController()
    // Initial: search not started
    expect(ctrl.isSearching()).toBe(false)
    expect(ctrl.getState().searchHits).toBeNull()
    expect(ctrl.getState().searchStarted).toBe(false)

    // Empty hits — still a "started" search
    ctrl.setSearchStarted(true)
    ctrl.setSearchHits([])
    expect(ctrl.isSearching()).toBe(true)
    expect(ctrl.getState().searchHits).toEqual([])
    expect(ctrl.getState().searchStarted).toBe(true)

    // No hits vs null are observably different states
    ctrl.setSearchHits(null)
    expect(ctrl.getState().searchHits).toBeNull()
    ctrl.setSearchStarted(false)
    expect(ctrl.isSearching()).toBe(false)
  })

  it('delete removes from UI immediately and calls backend after 3 seconds', async () => {
    const { ctrl, delSpy } = makeController()
    expect(ctrl.counts().all).toBe(5)

    ctrl.requestDelete('a')
    // UI removed immediately
    expect(ctrl.counts().all).toBe(4)
    expect(ctrl.filtered().find((e) => e.entry.id === 'a')).toBeUndefined()
    // Backend not called yet
    expect(delSpy.onDelete).not.toHaveBeenCalled()

    // Advance 3 seconds
    await vi.advanceTimersByTimeAsync(3000)
    expect(delSpy.onDelete).toHaveBeenCalledTimes(1)
    expect(delSpy.onDelete.mock.calls[0]![0]).toBe('a')
  })

  it('undo click restores original position and does not call backend', () => {
    const { ctrl, delSpy, notifySpy } = makeController()
    const original = ctrl.filtered().slice()
    expect(original[0]!.entry.id).toBe('a')

    ctrl.requestDelete('a')
    expect(ctrl.filtered().find((e) => e.entry.id === 'a')).toBeUndefined()

    // Notify should have been called with an undo callback
    expect(notifySpy.notify).toHaveBeenCalledTimes(1)
    const notifyArgs = notifySpy.notify.mock.calls[0]!
    const undoFn = notifyArgs[2] as (() => void) | undefined
    expect(typeof undoFn).toBe('function')

    undoFn!()

    // Restored to original position (first)
    const restored = ctrl.filtered()
    expect(restored[0]!.entry.id).toBe('a')
    // Backend must NOT have been called
    expect(delSpy.onDelete).not.toHaveBeenCalled()

    // Flushing timers also must not invoke delete (timer cancelled)
    vi.runAllTimers()
    expect(delSpy.onDelete).not.toHaveBeenCalled()
  })

  it('backend delete failure restores entry and notifies error', async () => {
    const { ctrl, delSpy, notifySpy } = makeController({ deleteResult: 'reject' })
    expect(ctrl.counts().all).toBe(5)

    ctrl.requestDelete('a')
    expect(ctrl.counts().all).toBe(4)

    await vi.advanceTimersByTimeAsync(3000)

    // Backend was called
    expect(delSpy.onDelete).toHaveBeenCalledTimes(1)
    // Entry restored
    expect(ctrl.counts().all).toBe(5)
    expect(ctrl.filtered().find((e) => e.entry.id === 'a')).toBeDefined()
    // Error notified
    const errorCall = notifySpy.notify.mock.calls.find(
      (c) => c[1] === 'error',
    )
    expect(errorCall).toBeDefined()
  })

  // --- additional safety tests for filter + counts combinations ---

  it('filter "credential" only shows credential entries', () => {
    const { ctrl } = makeController()
    ctrl.setFilter('credential')
    const filtered = ctrl.filtered()
    expect(filtered.length).toBe(2)
    expect(filtered.every((e) => e.entry.kind === 'credential')).toBe(true)
  })

  it('dispose cancels pending delete timers', async () => {
    const { ctrl, delSpy } = makeController({ deleteDelayMs: 3000 })
    ctrl.requestDelete('a')
    ctrl.dispose()
    await vi.advanceTimersByTimeAsync(5000)
    expect(delSpy.onDelete).not.toHaveBeenCalled()
  })
})
