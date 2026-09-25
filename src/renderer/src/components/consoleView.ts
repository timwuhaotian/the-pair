/**
 * Pure view helpers for the pair console / operations panel. No React imports
 * so they can be unit-tested under node.
 */
import type { buildTimeline } from '../lib/timeline'
import type { Pair, PairRunSummary, TurnCard } from '../store/usePairStore'

/** Distance (px) from the bottom within which the console keeps following new output. */
export const STICK_TO_BOTTOM_THRESHOLD_PX = 160

/**
 * Resolves the archived run being viewed. `viewingRunId` is global in the
 * store while run ids are per pair, so an id that isn't in *this* pair's
 * history (e.g. left over from another pair) is treated as "live".
 */
export function resolveViewingRun(
  runHistory: readonly PairRunSummary[],
  viewingRunId: string | null
): PairRunSummary | null {
  if (!viewingRunId) return null
  return runHistory.find((run) => run.id === viewingRunId) ?? null
}

export function isNearBottom(
  metrics: { scrollHeight: number; scrollTop: number; clientHeight: number },
  threshold: number = STICK_TO_BOTTOM_THRESHOLD_PX
): boolean {
  return metrics.scrollHeight - metrics.scrollTop - metrics.clientHeight < threshold
}

/**
 * The live turn card is hidden once its final message has landed in the
 * transcript (last message comes from the same role). Compare against the
 * UNFILTERED live messages — a role filter must not hide the other role's
 * in-progress card behind a "thinking" placeholder.
 */
export function selectVisibleTurnCard(
  liveMessages: ReadonlyArray<{ from: string }>,
  card: TurnCard | undefined
): TurnCard | null {
  if (!card) return null
  const last = liveMessages[liveMessages.length - 1]
  return !last || last.from !== card.role ? card : null
}

export interface CompositionSplit {
  before: string
  composing: string
  after: string
}

/**
 * Splits the console input into text before / inside / after the active IME
 * composition so the overlay can underline the composing slice. The textarea
 * value (and so `value`) usually already contains the composing text; it is
 * only inserted when missing, so it is never rendered twice.
 */
export function splitComposition(
  value: string,
  composing: string,
  start: number | null
): CompositionSplit {
  if (!composing || start === null) return { before: value, composing: '', after: '' }
  const at = Math.min(Math.max(start, 0), value.length)
  if (value.slice(at, at + composing.length) === composing) {
    return {
      before: value.slice(0, at),
      composing,
      after: value.slice(at + composing.length)
    }
  }
  return { before: value.slice(0, at), composing, after: value.slice(at) }
}

export type TimelineSource = Parameters<typeof buildTimeline>[1]

type LivePairFields = Pick<
  Pair,
  | 'name'
  | 'spec'
  | 'mentorModel'
  | 'executorModel'
  | 'status'
  | 'messages'
  | 'latestAcceptance'
  | 'modifiedFiles'
  | 'currentRunStartedAt'
  | 'currentRunFinishedAt'
>

/**
 * Builds the timeline/report input for either the live run or an archived
 * run. Archived runs map their own `startedAt`/`finishedAt`; they don't record
 * a modified-file list, so none is reported rather than the live run's files.
 */
export function buildTimelineSource(
  pair: LivePairFields,
  run: PairRunSummary | null
): TimelineSource {
  if (!run) {
    return {
      name: pair.name,
      spec: pair.spec,
      mentorModel: pair.mentorModel,
      executorModel: pair.executorModel,
      status: pair.status,
      messages: pair.messages,
      latestAcceptance: pair.latestAcceptance,
      modifiedFiles: pair.modifiedFiles,
      currentRunStartedAt: pair.currentRunStartedAt,
      currentRunFinishedAt: pair.currentRunFinishedAt
    }
  }

  const lastMessageAt = run.messages.reduce<number | undefined>(
    (latest, message) =>
      latest === undefined || message.timestamp > latest ? message.timestamp : latest,
    undefined
  )

  return {
    name: pair.name,
    spec: run.spec,
    mentorModel: run.mentorModel,
    executorModel: run.executorModel,
    status: run.status,
    messages: run.messages,
    latestAcceptance: run.latestAcceptance,
    modifiedFiles: [],
    currentRunStartedAt: run.startedAt,
    currentRunFinishedAt: run.finishedAt ?? lastMessageAt
  }
}
