import type { DockEntry } from '$lib/types/dock'
import { messages } from '$lib/i18n'

const CODE_KEYWORDS_RE =
  /^(import\s|function\s|const\s|let\s|var\s|class\s|def\s|return\s|from\s|export\s|require\()/m

const SPECIAL_CHARS_RE = /[{}()[\]=><;:]/g

/**
 * Detects if text content looks like code by checking for common keywords
 * on the first non-empty line and counting special characters in the first
 * two lines.
 */
export function looksLikeCode(text: string): boolean {
  const lines = text.split('\n')
  const firstNonEmpty = lines.find((l) => l.trim().length > 0)

  if (firstNonEmpty) {
    if (CODE_KEYWORDS_RE.test(firstNonEmpty)) return true
  }

  const firstTwo = lines.slice(0, 2).join('\n')
  const specialCount = (firstTwo.match(SPECIAL_CHARS_RE) ?? []).length
  return specialCount > 5
}

/** Pads a number to 2 digits with leading zero. */
function pad2(n: number): string {
  return n.toString().padStart(2, '0')
}

/** Formats an ISO date string as MM/DD HH:mm. */
function formatDate(iso: string): string {
  const d = new Date(iso)
  return `${pad2(d.getMonth() + 1)}/${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}`
}

const MAX_TITLE_LEN = 28

/**
 * Generates a display title from entry content based on entry kind.
 * Pure function — no side effects or DOM access.
 */
export function generateTitle(entry: DockEntry): string {
  if (entry.kind === 'image') {
    const label =
      entry.source === 'clipboard'
        ? messages.entry.pastedImage
        : entry.source === 'drop'
          ? messages.entry.droppedImage
          : messages.entry.imageShort
    return `${label} ${formatDate(entry.createdAt)}`
  }

  if (entry.kind === 'file') {
    return entry.fileName ?? messages.entry.unnamedFile
  }

  // kind === 'text'
  const content = entry.content ?? ''

  if (looksLikeCode(content)) {
    const lines = content.split('\n')
    const meaningful = lines.find((l) => l.trim().length > 0)
    return (meaningful?.trim() ?? '').slice(0, MAX_TITLE_LEN)
  }

  // Join soft-wrapped lines into the first paragraph.
  const lines = content.split('\n')
  const paragraphLines: string[] = []
  for (const line of lines) {
    if (line.trim() === '' && paragraphLines.length > 0) break
    if (line.trim() !== '' || paragraphLines.length > 0) {
      paragraphLines.push(line)
    }
  }
  const paragraph = paragraphLines.join(' ').replace(/\s+/g, ' ').trim()
  return paragraph.slice(0, MAX_TITLE_LEN)
}
