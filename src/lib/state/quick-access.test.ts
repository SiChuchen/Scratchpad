// src/lib/state/quick-access.test.ts
//
// Quick-access 面板的纯函数状态测试。
//
// 覆盖：
//   * Ctrl+Tab 在 record / search 模式间切换；
//   * 普通 Tab 不触发模式切换（允许焦点遍历）；
//   * Escape 触发 requestHide 回调；
//   * 重新 show 后保留 mode / draft / query / selectedId（hide/show 不销毁状态）。

import { describe, expect, it, vi } from 'vitest'
import { createQuickAccessState, handleKeydown } from './quick-access'
import type { QuickAccessState } from '$lib/types/quick-access'

function baseState(overrides: Partial<QuickAccessState> = {}): QuickAccessState {
  return {
    mode: 'record',
    draft: '',
    query: '',
    selectedId: null,
    ...overrides,
  }
}

describe('quick-access state', () => {
  it('Ctrl+Tab toggles between record and search', () => {
    const hide = vi.fn()
    const state = createQuickAccessState(baseState({ mode: 'record' }))
    handleKeydown(
      state,
      new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true }),
      hide,
    )
    expect(state.mode).toBe('search')

    handleKeydown(
      state,
      new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true }),
      hide,
    )
    expect(state.mode).toBe('record')
  })

  it('plain Tab does not toggle mode', () => {
    const hide = vi.fn()
    const state = createQuickAccessState(baseState({ mode: 'record' }))
    handleKeydown(state, new KeyboardEvent('keydown', { key: 'Tab' }), hide)
    expect(state.mode).toBe('record')
  })

  it('Escape requests hide', () => {
    const hide = vi.fn()
    const state = createQuickAccessState(baseState())
    handleKeydown(state, new KeyboardEvent('keydown', { key: 'Escape' }), hide)
    expect(hide).toHaveBeenCalledTimes(1)
  })

  it('re-show preserves mode, draft, query, selectedId', () => {
    const hide = vi.fn()
    const state = createQuickAccessState(
      baseState({
        mode: 'search',
        draft: '录入草稿',
        query: '搜索关键词',
        selectedId: 'entry-42',
      }),
    )

    // Simulate the user blurring the window (window.hide() preserves webview).
    hide.mockImplementation(() => {
      /* no-op: hide is a no-op in tests; state lives on the controller */
    })
    handleKeydown(state, new KeyboardEvent('keydown', { key: 'Escape' }), hide)

    // State must survive the hide/show cycle.
    expect(state.mode).toBe('search')
    expect(state.draft).toBe('录入草稿')
    expect(state.query).toBe('搜索关键词')
    expect(state.selectedId).toBe('entry-42')
  })
})
