import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { loadLocale } from '$lib/i18n'
import QuickAccessWindowBar from './QuickAccessWindowBar.svelte'

afterEach(() => {
  cleanup()
  loadLocale('zh-CN')
})

describe('QuickAccessWindowBar', () => {
  it('exposes pin and close actions', async () => {
    loadLocale('zh-CN')
    const onTogglePin = vi.fn()
    const onHide = vi.fn()
    render(QuickAccessWindowBar, {
      pinned: true,
      onTogglePin,
      onHide,
      onDrag: vi.fn(),
    })

    await fireEvent.click(screen.getByRole('button', { name: '取消置顶' }))
    await fireEvent.click(screen.getByRole('button', { name: '关闭' }))
    expect(onTogglePin).toHaveBeenCalledTimes(1)
    expect(onHide).toHaveBeenCalledTimes(1)
  })

  it('starts dragging only from non-button title space', async () => {
    loadLocale('zh-CN')
    const onDrag = vi.fn()
    render(QuickAccessWindowBar, {
      pinned: false,
      onTogglePin: vi.fn(),
      onHide: vi.fn(),
      onDrag,
    })

    await fireEvent.mouseDown(screen.getByTestId('quick-access-drag-region'))
    expect(onDrag).toHaveBeenCalledTimes(1)
    await fireEvent.mouseDown(screen.getByRole('button', { name: '置顶' }))
    expect(onDrag).toHaveBeenCalledTimes(1)
  })

  it('blocks repeated pin activation while pending', async () => {
    loadLocale('zh-CN')
    const onTogglePin = vi.fn()
    render(QuickAccessWindowBar, {
      pinned: true,
      pinPending: true,
      onTogglePin,
      onHide: vi.fn(),
      onDrag: vi.fn(),
    })

    const pin = screen.getByRole('button', { name: '取消置顶' })
    expect(pin).toBeDisabled()
    await fireEvent.click(pin)
    expect(onTogglePin).not.toHaveBeenCalled()
  })
})
