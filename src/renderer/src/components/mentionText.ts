/**
 * Pure text helpers for the `@file` and `/skill` mention popovers.
 */

export interface MentionReplacement {
  value: string
  cursor: number
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
  const at = before.lastIndexOf('@')
  if (at === -1) return null
  if (/\s/.test(before.slice(at + 1))) return null
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
