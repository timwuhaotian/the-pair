/**
 * Pure transcript-search matching: case-insensitive substring only.
 * No regex, no fuzzy — the query is always literal text.
 */

export function normalizeQuery(query: string): string {
  return query.trim().toLowerCase()
}

/** Messages whose content contains the query. Empty query returns the input unchanged. */
export function filterMessagesByQuery<T extends { content: string }>(
  messages: readonly T[],
  query: string
): readonly T[] {
  const q = normalizeQuery(query)
  if (!q) return messages
  return messages.filter((m) => m.content.toLowerCase().includes(q))
}

/** All non-overlapping [start, end) ranges of the query in text, case-insensitive. */
export function findMatchRanges(text: string, query: string): Array<[number, number]> {
  const q = normalizeQuery(query)
  if (!q || !text) return []
  const haystack = text.toLowerCase()
  const ranges: Array<[number, number]> = []
  let from = 0
  for (;;) {
    const start = haystack.indexOf(q, from)
    if (start === -1) return ranges
    ranges.push([start, start + q.length])
    from = start + q.length
  }
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
