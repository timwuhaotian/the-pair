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
 * Enter only picks a skill when the pick is unambiguous: the user moved the
 * highlight with the arrow keys, typed nothing after the `/`, or the typed
 * text is part of the skill's name. Otherwise Enter keeps its normal meaning
 * (submit) — so typing ` /tmp/out` or ` /usr` never gets swapped for a
 * fuzzy-matched skill.
 */
export function shouldAcceptSkillOnEnter(
  query: string,
  skillName: string,
  navigated: boolean
): boolean {
  if (navigated) return true
  const q = query.trim().toLowerCase()
  if (!q) return true
  return skillName.toLowerCase().includes(q)
}
