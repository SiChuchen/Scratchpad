// src/lib/state/quick-access.ts
//
// Quick-access 面板的前端状态协调器（纯逻辑，不依赖 Svelte）。
//
// 行为契约：
//   * `mode` 在 'record' / 'search' 间切换，由 Ctrl+Tab 触发；普通 Tab 不切换。
//   * Escape 触发 requestHide 回调（由调用方决定如何隐藏窗口——通常是
//     `getCurrentWindow().hide()`）。
//   * hide/show 不销毁 webview，因此 draft / query / selectedId 都保留；
//     调用方只持有同一份 state 对象，hide/show 不需要主动持久化。
//
// 这一层不直接调用 Tauri API（除显式导出的 `requestHide` 外），便于单测。

export type { QuickAccessMode, QuickAccessState } from '$lib/types/quick-access'

import type { QuickAccessState } from '$lib/types/quick-access'

export function createQuickAccessState(initial: Partial<QuickAccessState> = {}): QuickAccessState {
  return {
    mode: initial.mode ?? 'record',
    draft: initial.draft ?? '',
    query: initial.query ?? '',
    selectedId: initial.selectedId ?? null,
  }
}

/** 在 record / search 间切换；纯函数，返回新 mode 值。 */
export function toggleMode(
  mode: QuickAccessState['mode'],
): QuickAccessState['mode'] {
  return mode === 'record' ? 'search' : 'record'
}

/**
 * 处理键盘事件。直接在传入的 state 上做变更（mode 切换），并调用 `requestHide`
 * 回调来触发 Escape 行为。
 *
 * - Ctrl+Tab：切换 mode，并 preventDefault 避免 Tab 的默认焦点移动。
 * - Escape：调用 requestHide()。
 * - 普通 Tab：不处理（允许正常的焦点遍历）。
 */
export function handleKeydown(
  state: QuickAccessState,
  event: KeyboardEvent,
  requestHide: () => void,
): void {
  if (event.ctrlKey && event.key === 'Tab') {
    event.preventDefault()
    state.mode = toggleMode(state.mode)
    return
  }
  if (event.key === 'Escape') {
    requestHide()
  }
}
