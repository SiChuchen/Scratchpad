// src/lib/utils/shortcut-capture.test.ts
//
// Regression tests for multi-modifier shortcut parsing.
//
// 历史 bug: `keyEventToShortcutString` 把 modifiers 和 key 拼成单个
// 字符串（如 "Ctrl+Alt+V"），然后 handleKeyCapture 用 split('+') 拆分，
// 对于多修饰键场景会把第一个 "+" 之后的所有内容当作 key，
// 导致 `Alt+Shift+V` 被错误地解析成 modifiers="Alt" / key="Shift+V"，
// 后端 parse_key_code("Shift+V") 失败。这些测试确保解析返回结构化
// {modifiers, key}，不再做字符串往返。

import { describe, expect, it } from 'vitest'
import { captureShortcutFromEvent } from './shortcut-capture'

function makeEvent(init: Partial<KeyboardEvent> & { key: string }): KeyboardEvent {
  return new KeyboardEvent('keydown', {
    key: init.key,
    ctrlKey: init.ctrlKey ?? false,
    altKey: init.altKey ?? false,
    shiftKey: init.shiftKey ?? false,
    metaKey: init.metaKey ?? false,
  })
}

describe('captureShortcutFromEvent', () => {
  it('parses default Alt+Shift+V into {modifiers: "Alt+Shift", key: "V"}', () => {
    // 这是产品的默认快捷键；曾经因字符串拆分错误而无法被用户重新录制。
    const result = captureShortcutFromEvent(
      makeEvent({ key: 'v', altKey: true, shiftKey: true }),
    )
    expect(result).toEqual({ modifiers: 'Alt+Shift', key: 'V' })
  })

  it('parses Ctrl+Alt+V into {modifiers: "Ctrl+Alt", key: "V"}', () => {
    const result = captureShortcutFromEvent(
      makeEvent({ key: 'v', ctrlKey: true, altKey: true }),
    )
    expect(result).toEqual({ modifiers: 'Ctrl+Alt', key: 'V' })
  })

  it('parses Ctrl+Shift+V into {modifiers: "Ctrl+Shift", key: "V"}', () => {
    const result = captureShortcutFromEvent(
      makeEvent({ key: 'v', ctrlKey: true, shiftKey: true }),
    )
    expect(result).toEqual({ modifiers: 'Ctrl+Shift', key: 'V' })
  })

  it('parses three-modifier Ctrl+Alt+Shift+X', () => {
    const result = captureShortcutFromEvent(
      makeEvent({ key: 'x', ctrlKey: true, altKey: true, shiftKey: true }),
    )
    expect(result).toEqual({ modifiers: 'Ctrl+Alt+Shift', key: 'X' })
  })

  it('parses single-modifier Ctrl+V', () => {
    const result = captureShortcutFromEvent(
      makeEvent({ key: 'v', ctrlKey: true }),
    )
    expect(result).toEqual({ modifiers: 'Ctrl', key: 'V' })
  })

  it('parses Alt+Shift+Space (default quick-access shortcut)', () => {
    const result = captureShortcutFromEvent(
      makeEvent({ key: ' ', altKey: true, shiftKey: true }),
    )
    expect(result).toEqual({ modifiers: 'Alt+Shift', key: 'Space' })
  })

  it('maps Meta to Super', () => {
    const result = captureShortcutFromEvent(
      makeEvent({ key: 'k', metaKey: true }),
    )
    expect(result).toEqual({ modifiers: 'Super', key: 'K' })
  })

  it('parses function keys like F5', () => {
    const result = captureShortcutFromEvent(
      makeEvent({ key: 'F5', ctrlKey: true }),
    )
    expect(result).toEqual({ modifiers: 'Ctrl', key: 'F5' })
  })

  it('ignores bare Tab', () => {
    expect(captureShortcutFromEvent(makeEvent({ key: 'Tab' }))).toBeNull()
  })

  it('still captures Tab when combined with modifier (Ctrl+Tab)', () => {
    const result = captureShortcutFromEvent(
      makeEvent({ key: 'Tab', ctrlKey: true }),
    )
    expect(result).toEqual({ modifiers: 'Ctrl', key: 'Tab' })
  })

  it('returns null for plain keys without modifiers', () => {
    expect(captureShortcutFromEvent(makeEvent({ key: 'v' }))).toBeNull()
  })

  it('returns null for a lone modifier press', () => {
    expect(captureShortcutFromEvent(makeEvent({ key: 'Control', ctrlKey: true }))).toBeNull()
    expect(captureShortcutFromEvent(makeEvent({ key: 'Alt', altKey: true }))).toBeNull()
    expect(captureShortcutFromEvent(makeEvent({ key: 'Shift', shiftKey: true }))).toBeNull()
    expect(captureShortcutFromEvent(makeEvent({ key: 'Meta', metaKey: true }))).toBeNull()
  })

  it('returns null for unsupported multi-char keys (e.g. Unidentified)', () => {
    expect(
      captureShortcutFromEvent(makeEvent({ key: 'Unidentified', ctrlKey: true })),
    ).toBeNull()
  })

  it('uppercases single-letter keys', () => {
    const result = captureShortcutFromEvent(
      makeEvent({ key: 'a', ctrlKey: true }),
    )
    expect(result?.key).toBe('A')
  })

  it('uppercases digit keys', () => {
    const result = captureShortcutFromEvent(
      makeEvent({ key: '5', ctrlKey: true }),
    )
    expect(result?.key).toBe('5')
  })
})
