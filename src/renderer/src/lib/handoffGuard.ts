import type { PairStatus } from '../types'
import { normalizePairStatus } from './pairStatus'
import { extractErrorMessage } from './utils'

export interface HandoffEventLike {
  pairStatus?: string | null
  backendStatus?: string | null
  pairId?: string
  nextRole?: string
}

/** A queued `pair:handoff` payload waiting for its turn to run. */
export interface QueuedHandoff {
  pairId: string
  nextRole: string
}

/** Bounded per-pair handoff backlog. First handoff runs, the rest wait. */
const HANDOFF_QUEUE_LIMIT = 8

/**
 * Pure bounded-enqueue for handoffs. A second `pair:handoff` arriving while one
 * is in flight waits in the queue instead of being dropped; the runner drains
 * the queue when each attempt settles (HANDOFF_IGNORED or assign failure
 * included), so a swallowed first handoff never loses the second.
 *
 * Returns the queue with the event appended, or the queue unchanged when it is
 * already at the bound. Never mutates the input.
 */
export function tryEnqueueHandoff(
  queue: readonly QueuedHandoff[],
  event: QueuedHandoff
): QueuedHandoff[] {
  const invalid = !event.pairId || !event.nextRole || queue.length >= HANDOFF_QUEUE_LIMIT
  return invalid ? [...queue] : [...queue, { pairId: event.pairId, nextRole: event.nextRole }]
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

export interface ChecklistItemLike {
  description?: string | null
  completed?: boolean | null
}

/**
 * Renders a plan checklist as `- [ ]`/`- [x]` lines for an executor handoff
 * prompt, or `null` when empty so the caller splices nothing. This is the
 * renderer's counterpart to the backend checklist parser.
 */
export function formatChecklistForHandoff(items: readonly ChecklistItemLike[]): string | null {
  const lines = items.flatMap((item) => {
    const description = item.description?.trim()
    if (!description) return []
    return [`- [${item.completed ? 'x' : ' '}] ${description}`]
  })
  return lines.length > 0 ? lines.join('\n') : null
}

/**
 * Splices a rendered checklist into an executor handoff prompt. Returns the
 * prompt unchanged when the checklist is empty. Append, never prepend: the
 * plan header stays first so the executor still sees PLAN before the list.
 */
export function appendChecklistToExecutorPrompt(
  prompt: string,
  checklistSection: string | null | undefined
): string {
  const section = checklistSection?.trim()
  if (!section) return prompt
  return `${prompt}\n\nCHECKLIST\n${section}`
}

/**
 * True for the backend's "this handoff arrived after the pair stopped" rejection.
 * It is an expected race, not a failure: callers drop it silently.
 */
export function isHandoffIgnoredError(error: unknown): boolean {
  return extractErrorMessage(error, '').trimStart().startsWith(HANDOFF_IGNORED_PREFIX)
}
