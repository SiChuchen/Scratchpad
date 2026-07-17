import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import type { VaultEntryDetail, VaultEntrySummary } from '$lib/types/vault'
import { loadLocale } from '$lib/i18n'

const mocks = vi.hoisted(() => ({
  listEntries: vi.fn(),
  getEntry: vi.fn(),
  getLlmConfig: vi.fn(),
  getAiSettings: vi.fn(),
  updateEntry: vi.fn(),
  createEntry: vi.fn(),
  deleteEntry: vi.fn(),
  removeAiTag: vi.fn(),
  copyText: vi.fn(),
  searchLocal: vi.fn(),
  planSearch: vi.fn(),
  cancelSearch: vi.fn(),
  onTagsUpdated: vi.fn(),
  onLlmError: vi.fn(),
  listen: vi.fn(),
  invoke: vi.fn(),
}))

vi.mock('$lib/api/vault', () => ({
  vaultApi: {
    listEntries: mocks.listEntries,
    getEntry: mocks.getEntry,
    getLlmConfig: mocks.getLlmConfig,
    getAiSettings: mocks.getAiSettings,
    updateEntry: mocks.updateEntry,
    createEntry: mocks.createEntry,
    deleteEntry: mocks.deleteEntry,
    removeAiTag: mocks.removeAiTag,
    copyText: mocks.copyText,
    searchLocal: mocks.searchLocal,
    planSearch: mocks.planSearch,
    cancelSearch: mocks.cancelSearch,
  },
  onTagsUpdated: mocks.onTagsUpdated,
  onLlmError: mocks.onLlmError,
}))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))

import VaultView from './VaultView.svelte'

function detail(id: string, title: string): VaultEntryDetail {
  return {
    entry: {
      id,
      kind: 'note',
      title,
      notes: null,
      createdAt: '2026-07-17T00:00:00Z',
      updatedAt: '2026-07-17T00:00:00Z',
    },
    fields: [],
    tags: [],
    aiMetadata: null,
  }
}

function summary(value: VaultEntryDetail): VaultEntrySummary {
  return {
    entry: value.entry,
    tags: value.tags,
    preview: null,
  }
}

const entryA = detail('entry-a', 'Entry A')
const entryB = detail('entry-b', 'Entry B')

beforeEach(() => {
  mocks.listEntries.mockResolvedValue([summary(entryA), summary(entryB)])
  mocks.getEntry.mockImplementation(async (id: string) =>
    id === 'entry-a' ? entryA : entryB,
  )
  mocks.getLlmConfig.mockResolvedValue(null)
  mocks.getAiSettings.mockResolvedValue({
    autoEnrich: false,
    autoHybridSearch: false,
    sensitiveClipboardClearSeconds: null,
  })
  mocks.updateEntry.mockResolvedValue(entryB)
  mocks.createEntry.mockResolvedValue(entryA)
  mocks.deleteEntry.mockResolvedValue(undefined)
  mocks.removeAiTag.mockResolvedValue(undefined)
  mocks.copyText.mockResolvedValue(undefined)
  mocks.searchLocal.mockResolvedValue([])
  mocks.planSearch.mockResolvedValue(null)
  mocks.cancelSearch.mockResolvedValue(undefined)
  mocks.onTagsUpdated.mockResolvedValue(vi.fn())
  mocks.onLlmError.mockResolvedValue(vi.fn())
  mocks.listen.mockResolvedValue(vi.fn())
  mocks.invoke.mockResolvedValue(undefined)
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  loadLocale('zh-CN')
})

describe('VaultView', () => {
  it('replaces editor state when the user selects another entry', async () => {
    render(VaultView, { notify: vi.fn() })

    await fireEvent.click(await screen.findByRole('button', { name: '编辑 Entry A' }))
    expect(await screen.findByDisplayValue('Entry A')).toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: '编辑 Entry B' }))
    const titleInput = await screen.findByDisplayValue('Entry B')
    expect(screen.queryByDisplayValue('Entry A')).not.toBeInTheDocument()

    await fireEvent.submit(titleInput.closest('form')!)
    await waitFor(() => {
      expect(mocks.updateEntry).toHaveBeenCalledWith(
        'entry-b',
        expect.objectContaining({ title: 'Entry B' }),
      )
    })
  })

  it('uses English notifications and controls in English mode', async () => {
    loadLocale('en')
    const notify = vi.fn()
    render(VaultView, { notify })

    expect(await screen.findByRole('button', { name: 'Open quick access' })).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Edit Entry B' }))
    const titleInput = await screen.findByDisplayValue('Entry B')
    await fireEvent.submit(titleInput.closest('form')!)

    await waitFor(() => {
      expect(notify).toHaveBeenCalledWith('Saved', 'success')
    })
  })
})
