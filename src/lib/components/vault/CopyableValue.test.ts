// src/lib/components/vault/CopyableValue.test.ts
//
// CopyableValue 行为测试，覆盖 Task 12 要求的所有验收点：
//   * 敏感值默认掩码；
//   * 眼睛按钮只切换自己那一行；
//   * 复制不需要先 reveal；
//   * 复制回调 payload 包含 label/value/sensitive；
//   * window blur 后重新掩码；
//   * resetToken 变化后重新掩码；
//   * eye 与 copy 按钮都有 aria-label；
//   * copy 的 aria-label 不暴露 value。

import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'

import CopyableValue from './CopyableValue.svelte'
import TwoCopyableValues from './TwoCopyableValues.test.svelte'

afterEach(() => cleanup())

describe('CopyableValue', () => {
  it('masks one sensitive value and reveals only that row', async () => {
    const onCopy = vi.fn()
    render(TwoCopyableValues, { onCopy })

    expect(screen.queryByText('secret-a')).not.toBeInTheDocument()
    expect(screen.queryByText('secret-b')).not.toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: '显示 密码' }))

    expect(screen.getByText('secret-a')).toBeInTheDocument()
    expect(screen.queryByText('secret-b')).not.toBeInTheDocument()
  })

  it('allows copy without revealing and payload includes actual value', async () => {
    const onCopy = vi.fn()
    render(CopyableValue, {
      label: '密码',
      value: 'secret-a',
      sensitive: true,
      onCopy,
    })

    // 仍处于掩码状态
    expect(screen.queryByText('secret-a')).not.toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: /复制 密码/ }))

    expect(onCopy).toHaveBeenCalledTimes(1)
    expect(onCopy).toHaveBeenCalledWith({
      label: '密码',
      value: 'secret-a',
      sensitive: true,
    })
  })

  it('non-sensitive value still calls onCopy with sensitive=false', async () => {
    const onCopy = vi.fn()
    render(CopyableValue, {
      label: '用户名',
      value: 'alice',
      sensitive: false,
      onCopy,
    })

    await fireEvent.click(screen.getByRole('button', { name: /复制 用户名/ }))

    expect(onCopy).toHaveBeenCalledWith({
      label: '用户名',
      value: 'alice',
      sensitive: false,
    })
  })

  it('re-masks on window blur', async () => {
    render(CopyableValue, {
      label: '密码',
      value: 'secret-a',
      sensitive: true,
      onCopy: vi.fn(),
    })

    await fireEvent.click(screen.getByRole('button', { name: '显示 密码' }))
    expect(screen.getByText('secret-a')).toBeInTheDocument()

    window.dispatchEvent(new Event('blur'))
    // Svelte 5 effect flush
    await new Promise((r) => setTimeout(r, 0))

    expect(screen.queryByText('secret-a')).not.toBeInTheDocument()
  })

  it('re-masks when resetToken changes', async () => {
    const { rerender } = render(CopyableValue, {
      label: '密码',
      value: 'secret-a',
      sensitive: true,
      resetToken: 0,
      onCopy: vi.fn(),
    })

    await fireEvent.click(screen.getByRole('button', { name: '显示 密码' }))
    expect(screen.getByText('secret-a')).toBeInTheDocument()

    await rerender({ label: '密码', value: 'secret-a', sensitive: true, resetToken: 1, onCopy: vi.fn() })

    expect(screen.queryByText('secret-a')).not.toBeInTheDocument()
  })

  it('eye button has localized aria-label', () => {
    render(CopyableValue, {
      label: '密码',
      value: 'secret-a',
      sensitive: true,
      onCopy: vi.fn(),
    })

    expect(screen.getByRole('button', { name: '显示 密码' })).toBeInTheDocument()
  })

  it('copy button aria-label does NOT include the value', () => {
    render(CopyableValue, {
      label: '密码',
      value: 'super-secret-value',
      sensitive: true,
      onCopy: vi.fn(),
    })

    const copyBtn = screen.getByRole('button', { name: /复制/ })
    expect(copyBtn.getAttribute('aria-label')).not.toContain('super-secret-value')
  })

  it('shows a larger visible copy action in prominent mode', () => {
    render(CopyableValue, {
      label: '账号',
      value: 'alice',
      prominent: true,
      onCopy: vi.fn(),
    })

    const copy = screen.getByRole('button', { name: '复制 账号' })
    expect(copy).toHaveTextContent('复制')
    expect(copy).toHaveAttribute('data-prominent-action', 'copy')
  })

  it('keeps copy as the final action for sensitive values', () => {
    render(CopyableValue, {
      label: '密码',
      value: 'secret',
      sensitive: true,
      prominent: true,
      onCopy: vi.fn(),
    })

    const actions = screen.getByTestId('copyable-actions')
    expect(actions.lastElementChild).toBe(
      screen.getByRole('button', { name: '复制 密码' }),
    )
  })
})
