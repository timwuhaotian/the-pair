type PairSoundCueStatus =
  | 'Idle'
  | 'Mentoring'
  | 'Executing'
  | 'Reviewing'
  | 'Paused'
  | 'Awaiting Human Review'
  | 'Error'
  | 'Finished'

export type PairSoundCue = 'finish' | 'attention' | 'pause' | 'error' | null

export interface PairSoundCueInput {
  prevStatus: PairSoundCueStatus | undefined
  nextStatus: PairSoundCueStatus | undefined
  /** ms-since-epoch when the user manually clicked Pause for this pair. */
  manualPauseAt?: number | null
  /** Now timestamp injection for tests. Defaults to Date.now(). */
  now?: number
  /** Suppression window after a manual pause where Paused state events stay silent. */
  manualPauseWindowMs?: number
}

/**
 * Default suppression window: long enough to cover backend round-trip and React
 * commit, short enough that a real Paused transition minutes later still rings.
 */
export const DEFAULT_MANUAL_PAUSE_WINDOW_MS = 5000

/**
 * Decide which audio cue (if any) to play for a pair status transition.
 *
 * Rules:
 * - `Finished`           → `'finish'`    (mentor signaled TASK_COMPLETE)
 * - `Awaiting Human Review` → `'attention'` (mentor delivered a verdict that needs review)
 * - `Error`              → `'error'`     (run failed)
 * - `Paused` (auto)      → `'pause'`     (system paused itself; e.g. iteration budget)
 * - `Paused` (manual within window) → `null` (user-initiated; pause sound already fired)
 * - Same-status events (snapshot rehydration, no-op updates) → `null`
 * - Missing prev status (pair not yet in store) → `null` (avoid spurious chimes during boot)
 */
export function resolvePairSoundCue(input: PairSoundCueInput): PairSoundCue {
  const { prevStatus, nextStatus } = input
  if (!prevStatus || !nextStatus) return null
  if (prevStatus === nextStatus) return null

  switch (nextStatus) {
    case 'Finished':
      return 'finish'
    case 'Awaiting Human Review':
      return 'attention'
    case 'Error':
      return 'error'
    case 'Paused': {
      const now = input.now ?? Date.now()
      const manualPauseAt = input.manualPauseAt ?? null
      const window = input.manualPauseWindowMs ?? DEFAULT_MANUAL_PAUSE_WINDOW_MS
      if (manualPauseAt !== null && now - manualPauseAt >= 0 && now - manualPauseAt < window) {
        return null
      }
      return 'pause'
    }
    default:
      return null
  }
}
