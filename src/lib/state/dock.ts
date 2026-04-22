import type { DockEntry, EntryKind, EntryMembershipView } from '$lib/types/dock'

export function insertHomeEntry(entries: DockEntry[], entry: DockEntry): DockEntry[] {
  return [
    { ...entry, collapsed: false, inHome: true },
    ...entries.filter((item) => item.id !== entry.id),
  ]
}

export function filterHomeEntries(entries: DockEntry[], kind: EntryKind): DockEntry[] {
  return entries.filter((entry) => entry.kind === kind && entry.inHome)
}

export function removeEntryFromView(
  entries: DockEntry[],
  view: EntryMembershipView,
  entryId: string,
): DockEntry[] {
  return entries
    .map((entry) =>
      entry.id === entryId
        ? {
            ...entry,
            inHome: view === 'home' ? false : entry.inHome,
            inNote: view === 'note' ? false : entry.inNote,
          }
        : entry,
    )
    .filter((entry) => entry.inHome || entry.inNote)
}
