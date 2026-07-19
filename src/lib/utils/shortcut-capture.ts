// src/lib/utils/shortcut-capture.ts
//
// 把 KeyboardEvent 解析为快捷键的 {modifiers, key} 结构。
//
// 注意：早期实现把 modifiers 和 key 拼成单个字符串再 split('+')，
// 当 modifiers 本身包含 "+"（例如 "Ctrl+Alt"）时会导致错误的拆分，
// 让 `Alt+Shift+V` 被错误地解析成 modifiers="Alt" / key="Shift+V"，
// 后端 parse_key_code("Shift+V") 会失败。
//
// 因此这里直接返回结构化数据，不再做字符串往返。

/** 单个修饰键的稳定字符串表示，按 Ctrl / Alt / Shift / Super 排序。 */
export type ShortcutModifierName = 'Ctrl' | 'Alt' | 'Shift' | 'Super'

/** 解析后的快捷键；modifiers 用 "+" 连接的零或多个修饰键，key 为单个键名。 */
export interface CapturedShortcut {
  modifiers: string
  key: string
}

/** 不应被捕获的修饰键 KeyCode 名。 */
const MODIFIER_KEY_NAMES = new Set(['Alt', 'Control', 'Shift', 'Meta'])

/**
 * 把 KeyboardEvent 转换为快捷键描述。
 *
 * 行为：
 *   - 忽略 Tab（避免破坏焦点导航）
 *   - 至少需要一个修饰键，否则返回 null
 *   - 单独的修饰键（无主键）返回 null
 *   - 不支持的多字符键（非 F1-F12、非 Space / Enter / Tab）返回 null
 *   - 字母 / 数字统一转大写
 *   - 修饰键统一映射为 Ctrl / Alt / Shift / Super（Meta -> Super，与后端一致）
 *
 * 返回的对象保证 modifiers 与 key 互相独立，可安全地分别传给后端。
 */
export function captureShortcutFromEvent(e: KeyboardEvent): CapturedShortcut | null {
  // 不接收普通 Tab（避免破坏焦点导航）—— 这里特指裸 Tab
  if (e.key === 'Tab' && !e.altKey && !e.ctrlKey && !e.shiftKey && !e.metaKey) {
    return null
  }

  const hasMod = e.altKey || e.ctrlKey || e.shiftKey || e.metaKey
  if (!hasMod) return null

  // 单独的修饰键不构成快捷键
  if (MODIFIER_KEY_NAMES.has(e.key)) return null

  // 标准化主键
  let key: string
  if (e.key === ' ') {
    key = 'Space'
  } else if (e.key.length === 1) {
    // 字母 / 数字 / 标点：统一大写
    key = e.key.toUpperCase()
  } else if (e.key.startsWith('F') && /^F([1-9]|1[0-2])$/.test(e.key)) {
    // F1-F12
    key = e.key
  } else if (['Enter', 'Tab', 'Escape', 'Backspace', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(e.key)) {
    key = e.key
  } else {
    // 其他多字符键（如 "Unidentified"、IME composition 等）不支持
    return null
  }

  const mods: ShortcutModifierName[] = []
  if (e.ctrlKey) mods.push('Ctrl')
  if (e.altKey) mods.push('Alt')
  if (e.shiftKey) mods.push('Shift')
  if (e.metaKey) mods.push('Super')

  return {
    modifiers: mods.join('+'),
    key,
  }
}
