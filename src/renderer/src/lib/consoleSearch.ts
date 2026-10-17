/**
 * Pure transcript-search matching: case-insensitive substring only.
 * No regex, no fuzzy — the query is always literal text. Whitespace collapses
 * on both sides, matching how the browser renders it (see findMatchRanges),
 * except in literal mode for code under <pre>, where whitespace renders exact.
 */

export interface MatchOptions {
  /** Match whitespace exactly instead of collapsing runs (code under `<pre>`). */
  literal?: boolean
}

export function normalizeQuery(query: string): string {
  return query.trim().toLowerCase()
}

/**
 * All non-overlapping [start, end) ranges of the query in text, case-insensitive.
 * Whitespace collapses on both sides the way rendered HTML collapses it — a
 * soft line break and any run of spaces display as one space, so a query typed
 * off the visible text must still match the raw string. Ranges index the
 * original text and span the full whitespace runs they replace, so <mark>
 * wraps the whole rendered phrase. Each collapsed unit is lowercased on its
 * own so offsets stay aligned — `toLowerCase()` is not length-preserving
 * (e.g. U+0130 expands).
 *
 * With `literal: true` nothing collapses on either side: content inside
 * `<pre>` (fenced/indented code) renders newlines and space runs literally,
 * so a space-separated query must not match text split across lines there.
 */
export function findMatchRanges(
  text: string,
  query: string,
  options: MatchOptions = {}
): Array<[number, number]> {
  const literal = options.literal === true
  const q = literal ? normalizeQuery(query) : normalizeQuery(query).replace(/\s+/g, ' ')
  if (!q || !text) return []

  // Collapsed text with a per-unit map back to the original string: a
  // whitespace run becomes one ' ' unit spanning the whole run, any other
  // code unit keeps its own offsets (expanding if its lowercase form does).
  // In literal mode every code unit — whitespace included — maps itself.
  const units: string[] = []
  const unitStart: number[] = []
  const unitEnd: number[] = []
  let i = 0
  while (i < text.length) {
    if (!literal && /\s/.test(text[i]!)) {
      const runStart = i
      do {
        i += 1
      } while (i < text.length && /\s/.test(text[i]!))
      units.push(' ')
      unitStart.push(runStart)
      unitEnd.push(i)
      continue
    }
    const lower = text[i]!.toLowerCase()
    for (let u = 0; u < lower.length; u++) {
      units.push(lower[u]!)
      unitStart.push(i)
      unitEnd.push(i + 1)
    }
    i += 1
  }

  const collapsed = units.join('')
  const window = q.length
  const ranges: Array<[number, number]> = []
  let from = 0
  while (from + window <= collapsed.length) {
    if (collapsed.slice(from, from + window) === q) {
      ranges.push([unitStart[from]!, unitEnd[from + window - 1]!])
      from += window
    } else {
      from += 1
    }
  }
  return ranges
}

export function countOccurrences(text: string, query: string, options: MatchOptions = {}): number {
  return findMatchRanges(text, query, options).length
}

/** Wrap-around cycling over match ordinals. Returns -1 when there are no matches. */
export function nextMatchIndex(current: number, total: number, dir: 1 | -1): number {
  if (total <= 0) return -1
  if (current < 0) return dir === 1 ? 0 : total - 1
  return (current + dir + total) % total
}
