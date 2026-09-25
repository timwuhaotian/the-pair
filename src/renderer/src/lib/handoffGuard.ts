import type { PairStatus } from '../types'
import { normalizePairStatus } from './pairStatus'
import { extractErrorMessage } from './utils'

export interface HandoffEventLike {
  pairStatus?: string | null
  backendStatus?: string | null
}

/** Statuses in which a pending `pair:handoff` must not start the next turn. */
const STOPPED_STATUSES: ReadonlySet<PairStatus> = new Set<PairStatus>([
  'Finished',
  'Paused',
  'Error',
  'Awaiting Human Review'
])

function isStopped(status: string | null | undefined): boolean {
  const normalized = normalizePairStatus(status)
  return normalized !== undefined && STOPPED_STATUSES.has(normalized)
}

/**
 * True when a `pair:handoff` EVENT arrived for a pair that is no longer running.
 *
 * Only for the automatic event path: the plan gate stops in Awaiting Human Review
 * and the backend never emits a handoff there, so an event seen in that state is
 * stale and must not start the executor. The human's approve/reject decision
 * (`resolvePlanReview`) calls `assignTask` with a role directly and never consults
 * this guard.
 *
 * `backendStatus` comes straight from `pair_get_state` (kebab-case, e.g.
 * `"paused"`), `pairStatus` from the renderer mirror (PascalCase); both are
 * normalized before comparing.
 */
export function shouldIgnoreHandoffEvent({ pairStatus, backendStatus }: HandoffEventLike): boolean {
  return isStopped(backendStatus) || isStopped(pairStatus)
}

/** Prefix of the error `pair_assign_task` returns when it rejects a handoff for a stopped pair. */
export const HANDOFF_IGNORED_PREFIX = 'HANDOFF_IGNORED'

/**
 * True for the backend's "this handoff arrived after the pair stopped" rejection.
 * It is an expected race, not a failure: callers drop it silently.
 */
export function isHandoffIgnoredError(error: unknown): boolean {
  return extractErrorMessage(error, '').trimStart().startsWith(HANDOFF_IGNORED_PREFIX)
}
