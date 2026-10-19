/**
 * Pure text helpers for the `@file` and `/skill` mention popovers.
 */

export interface MentionReplacement {
  value: string
  cursor: number
}

const FILE_MENTION_RE = /(?:^|\s)@(\S*)$/

/**
 * The `@query` being typed at the end of `before`, or null when there is none.
 * The `@` must start a token, so an email address (`user@example.com`) never
 * opens the popover, while a later `@` inside a path (`@node_modules/@types`)
 * stays part of the query.
 */
export function findFileMentionQuery(before: string): string | null {
  return before.match(FILE_MENTION_RE)?.[1] ?? null
}

/**
 * Replaces the `@query` token that ends at `cursor` with `@path`. Returns null
 * when there is no such token any more (the text changed while the file was
 * being read), so the caller never splices the path into an arbitrary spot.
 */
export function replaceFileMentionToken(
  text: string,
  cursor: number,
  path: string
): MentionReplacement | null {
  const before = text.slice(0, cursor)
  const query = findFileMentionQuery(before)
  if (query === null) return null
  const at = before.length - query.length - 1
  return {
    value: `${text.slice(0, at)}@${path}${text.slice(cursor)}`,
    cursor: at + path.length + 1
  }
}

/**
 * Enter picks the highlighted skill, like Tab, unless the typed text looks like
 * a filesystem path (` /tmp/out`, ` /usr/local`, ` /~/x`) — then Enter keeps its
 * normal meaning (submit) instead of swapping the path for a fuzzy-matched
 * skill. Moving the highlight with the arrow keys always makes Enter pick.
 */
export function shouldAcceptSkillOnEnter(
  query: string,
  skillName: string,
  navigated: boolean
): boolean {
  if (navigated) return true
  const q = query.trim().toLowerCase()
  if (!q || skillName.toLowerCase().includes(q)) return true
  return !looksLikePath(q)
}

function looksLikePath(query: string): boolean {
  return /[/\\~]/.test(query) || query.startsWith('.')
}

/** Viewport rect of the caret line a mention popover anchors beside. */
export interface CaretLineRect {
  top: number
  bottom: number
  left: number
}

/** Fixed-position offsets for a mention popover: `top` opens below, `bottom` above. */
export type MentionPopupPosition = { top: number; left: number } | { bottom: number; left: number }

/** Overlap (px) between the popover edge and the caret line it sits beside. */
const POPUP_LINE_OVERLAP_PX = 4

/**
 * Opens the popover below the caret line when it fits, otherwise above — the
 * console input is docked at the bottom of the window, where a downward
 * popover would spill off-screen. When neither side fits, the roomier wins.
 * `popupHeight` is the rendered height, not the `max-h-*` cap: a short list
 * fits below a caret that a full-height list would not.
 */
export function placeMentionPopup(
  caret: CaretLineRect,
  viewportHeight: number,
  popupHeight: number
): MentionPopupPosition {
  const top = caret.top - POPUP_LINE_OVERLAP_PX
  const spaceBelow = viewportHeight - top
  const spaceAbove = caret.bottom + POPUP_LINE_OVERLAP_PX
  if (spaceBelow >= popupHeight || spaceBelow >= spaceAbove) {
    return { top, left: caret.left }
  }
  return { bottom: viewportHeight - spaceAbove, left: caret.left }
}
