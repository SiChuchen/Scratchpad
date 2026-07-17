import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { loadLocale } from '$lib/i18n'
import QuickAccessFab from './QuickAccessFab.svelte'

afterEach(cleanup)

describe('QuickAccessFab', () => {
  it('exposes and invokes the global action', async () => {
    loadLocale('zh-CN')
    const onOpen = vi.fn()
    render(QuickAccessFab, { onOpen })
    const button = screen.getByRole('button', { name: '打开全局快速入口' })
    await fireEvent.click(button)
    expect(onOpen).toHaveBeenCalledTimes(1)
  })

  it('blocks activation while opening', async () => {
    loadLocale('en')
    const onOpen = vi.fn()
    render(QuickAccessFab, { onOpen, disabled: true })
    const button = screen.getByRole('button', { name: 'Open quick access' })
    expect(button).toBeDisabled()
    await fireEvent.click(button)
    expect(onOpen).not.toHaveBeenCalled()
  })
})
