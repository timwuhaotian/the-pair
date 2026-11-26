/**
 * Structural sharing for state that arrives whole over IPC.
 *
 * The backend republishes a pair's full state (transcript, modified files)
 * about once a second while it runs. Deserializing gives every message and
 * file a new identity each time, which defeats `memo`, `useMemo` and effect
 * dependencies downstream. These helpers hand back the previous objects when
 * nothing in them changed, so only real changes propagate.
 */

function sameValue(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object') return false
  // Nested values (token usage, tool-call events, attachments) are small and
  // plain JSON from the backend; a serialized compare is exact for them.
  return JSON.stringify(a) === JSON.stringify(b)
}

/** True when two plain records hold equal values; an undefined field counts as absent. */
export function sameRecord(a: object, b: object): boolean {
  if (a === b) return true
  const left = a as Record<string, unknown>
  const right = b as Record<string, unknown>
  const keys = new Set([...Object.keys(left), ...Object.keys(right)])
  for (const key of keys) {
    if (!sameValue(left[key], right[key])) return false
  }
  return true
}

/**
 * `next`, with each item replaced by its previous object (matched by id) when
 * unchanged. Returns `prev` itself when the whole list is unchanged.
 */
export function reuseById<T extends { id: string }>(prev: readonly T[], next: readonly T[]): T[] {
  const previous = new Map(prev.map((item) => [item.id, item]))
  let changed = prev.length !== next.length
  const out = next.map((item, index) => {
    const old = previous.get(item.id)
    if (old !== undefined && sameRecord(old, item)) {
      if (prev[index] !== old) changed = true
      return old
    }
    changed = true
    return item
  })
  return changed ? out : (prev as T[])
}

/** `prev` when it holds the same records in the same order as `next`, else `next`. */
export function reuseList<T extends object>(prev: readonly T[], next: readonly T[]): T[] {
  if (prev.length === next.length && prev.every((item, index) => sameRecord(item, next[index]))) {
    return prev as T[]
  }
  return next as T[]
}
