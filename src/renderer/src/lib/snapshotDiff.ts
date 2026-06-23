import type { TurnTokenUsage } from '../types'

type SnapshotComparableTurnCard = {
  content: string
  state: 'live' | 'final'
  tokenUsage?: TurnTokenUsage
}

export type SnapshotComparablePair = {
  status: string
  turn: string
  iterations: number
  currentRunFinishedAt?: number
  currentRunStartedAt: number
  runCount: number
  pendingMentorModel?: string
  pendingExecutorModel?: string
  mentorModel: string
  executorModel: string
  currentTurnCard?: SnapshotComparableTurnCard
}

function areTokenUsagesEqual(
  first: TurnTokenUsage | undefined,
  second: TurnTokenUsage | undefined
): boolean {
  if (first === second) return true
  if (!first || !second) return false

  return (
    first.outputTokens === second.outputTokens &&
    first.inputTokens === second.inputTokens &&
    first.lastUpdatedAt === second.lastUpdatedAt &&
    first.source === second.source &&
    first.provider === second.provider
  )
}

export function shouldSaveSnapshot(
  previous: SnapshotComparablePair,
  next: SnapshotComparablePair
): boolean {
  const previousCard = previous.currentTurnCard
  const nextCard = next.currentTurnCard

  return (
    previous.status !== next.status ||
    previous.turn !== next.turn ||
    previous.iterations !== next.iterations ||
    previous.currentRunFinishedAt !== next.currentRunFinishedAt ||
    previous.currentRunStartedAt !== next.currentRunStartedAt ||
    previous.runCount !== next.runCount ||
    previous.pendingMentorModel !== next.pendingMentorModel ||
    previous.pendingExecutorModel !== next.pendingExecutorModel ||
    previous.mentorModel !== next.mentorModel ||
    previous.executorModel !== next.executorModel ||
    previousCard?.content !== nextCard?.content ||
    previousCard?.state !== nextCard?.state ||
    !areTokenUsagesEqual(previousCard?.tokenUsage, nextCard?.tokenUsage)
  )
}
