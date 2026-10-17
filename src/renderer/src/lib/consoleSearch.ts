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
 * Lowercased units of text[from,to) with offsets back into the original.
 * The slice is lowercased as one string so context-sensitive mappings apply
 * (Greek word-final Σ → ς, which per-character lowering loses), while offsets
 * are re-derived per code point because toLowerCase is not length-preserving
 * (U+0130 expands to two code units; surrogate pairs fold as one character).
 * The two views align: default case mapping differs from per-code-point
 * lowering only by in-place σ→ς substitutions, never by length.
 */
function pushLowerUnits(
  text: string,
  from: number,
  to: number,
  units: string[],
  unitStart: number[],
  unitEnd: number[]
): void {
  const lowered = text.slice(from, to).toLowerCase()
  let i = from
  let k = 0
  while (i < to) {
    const cp = text.codePointAt(i)!
    const cpLen = cp > 0xffff ? 2 : 1
    const cpLowerLen = String.fromCodePoint(cp).toLowerCase().length
    for (let u = 0; u < cpLowerLen; u++) {
      units.push(lowered[k + u]!)
      unitStart.push(i)
      unitEnd.push(i + cpLen)
    }
    i += cpLen
    k += cpLowerLen
  }
}

/**
 * All non-overlapping [start, end) ranges of the query in text, case-insensitive.
 * Whitespace collapses on both sides the way rendered HTML collapses it — a
 * soft line break and any run of spaces display as one space, so a query typed
 * off the visible text must still match the raw string. Ranges index the
 * original text and span the full whitespace runs they replace, so <mark>
 * wraps the whole rendered phrase. Case folding lowercases each run as one
 * string (see pushLowerUnits) so whole-string rules survive.
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
  // whitespace run becomes one ' ' unit spanning the whole run; everything
  // else lowers as whole runs. In literal mode the entire text is one run.
  const units: string[] = []
  const unitStart: number[] = []
  const unitEnd: number[] = []
  if (literal) {
    pushLowerUnits(text, 0, text.length, units, unitStart, unitEnd)
  } else {
    let i = 0
    while (i < text.length) {
      if (/\s/.test(text[i]!)) {
        const runStart = i
        do {
          i += 1
        } while (i < text.length && /\s/.test(text[i]!))
        units.push(' ')
        unitStart.push(runStart)
        unitEnd.push(i)
        continue
      }
      const runStart = i
      do {
        i += 1
      } while (i < text.length && !/\s/.test(text[i]!))
      pushLowerUnits(text, runStart, i, units, unitStart, unitEnd)
    }
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
