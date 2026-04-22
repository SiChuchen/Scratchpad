import { describe, expect, it } from 'vitest'
import { filterHomeEntries, insertHomeEntry, removeEntryFromView } from './dock'
import type { DockEntry } from '$lib/types/dock'

const textEntry = (id: string, kind: DockEntry['kind'] = 'text'): DockEntry => ({
  id,
  kind,
  content: kind === 'text' ? `body-${id}` : null,
  filePath: null,
  fileName: null,
  mimeType: null,
  width: null,
  height: null,
  sizeBytes: null,
  collapsed: true,
  inHome: true,
  inNote: false,
  source: 'manual',
  createdAt: '2026-04-19T00:00:00Z',
  updatedAt: '2026-04-19T00:00:00Z',
})

describe('dock state helpers', () => {
  it('inserts new home entries expanded at the top', () => {
    const entries = [textEntry('older')]
    const inserted = insertHomeEntry(entries, textEntry('newer'))
    expect(inserted[0].id).toBe('newer')
    expect(inserted[0].collapsed).toBe(false)
  })

  it('filters home entries by kind', () => {
    const entries = [textEntry('t1', 'text'), textEntry('f1', 'file')]
    expect(filterHomeEntries(entries, 'file')).toHaveLength(1)
  })

  it('removes only the targeted membership', () => {
    const noteShared = { ...textEntry('shared'), inNote: true }
    const removed = removeEntryFromView([noteShared], 'home', 'shared')
    expect(removed[0].inHome).toBe(false)
    expect(removed[0].inNote).toBe(true)
  })

  it('keeps categories scoped to home membership only', () => {
    const noteOnly = { ...textEntry('note-only', 'image'), inHome: false, inNote: true }
    expect(filterHomeEntries([noteOnly], 'image')).toHaveLength(0)
  })

  it('prepends imported image entries before older home entries', () => {
    const older = textEntry('older')
    const image = { ...textEntry('image', 'image'), filePath: 'C:/tmp/image.png' }
    const next = insertHomeEntry([older], image)
    expect(next.map((entry) => entry.id)).toEqual(['image', 'older'])
  })
})
