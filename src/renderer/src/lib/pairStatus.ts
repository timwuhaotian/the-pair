import type { PairStatus } from '../types'

export function isPairActive(status: PairStatus): boolean {
  return status === 'Mentoring' || status === 'Executing' || status === 'Reviewing'
}

export function isPairBusy(status: PairStatus): boolean {
  return isPairActive(status) || status === 'Awaiting Human Review'
}

/**
 * Map any status spelling to the renderer's canonical `PairStatus`.
 *
 * The backend serializes `PairStatus` as kebab-case (`"paused"`,
 * `"awaiting-human-review"`, and historically the literal
 * `"Awaiting Human Review"`), while the renderer uses PascalCase. Anything that
 * compares a status coming over IPC must go through this first.
 */
export function normalizePairStatus(raw: unknown): PairStatus | undefined {
  if (typeof raw !== 'string') return undefined
  const normalized = raw
    .trim()
    .toLowerCase()
    .replace(/[_\s]+/g, '-')

  switch (normalized) {
    case 'idle':
      return 'Idle'
    case 'mentoring':
      return 'Mentoring'
    case 'executing':
      return 'Executing'
    case 'reviewing':
      return 'Reviewing'
    case 'paused':
      return 'Paused'
    case 'awaiting-human-review':
      return 'Awaiting Human Review'
    case 'error':
      return 'Error'
    case 'finished':
      return 'Finished'
    default:
      return undefined
  }
}
