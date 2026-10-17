/**
 * Pure transcript-search matching: case-insensitive substring only.
 * No regex, no fuzzy — the query is always literal text.
 */

export function normalizeQuery(query: string): string {
  return query.trim().toLowerCase()
}

/**
 * All non-overlapping [start, end) ranges of the query in text, case-insensitive.
 * Each window is lowercased on its own so the ranges always index the original
 * string — `toLowerCase()` is not length-preserving (e.g. U+0130 expands).
 */
export function findMatchRanges(text: string, query: string): Array<[number, number]> {
  const q = normalizeQuery(query)
  const window = query.trim().length
  if (!q || !text || window === 0) return []
  const ranges: Array<[number, number]> = []
  let from = 0
  while (from + window <= text.length) {
    if (text.slice(from, from + window).toLowerCase() === q) {
      ranges.push([from, from + window])
      from += window
    } else {
      from += 1
    }
  }
  return ranges
}

export function countOccurrences(text: string, query: string): number {
  return findMatchRanges(text, query).length
}

/** Wrap-around cycling over match ordinals. Returns -1 when there are no matches. */
export function nextMatchIndex(current: number, total: number, dir: 1 | -1): number {
  if (total <= 0) return -1
  if (current < 0) return dir === 1 ? 0 : total - 1
  return (current + dir + total) % total
}
