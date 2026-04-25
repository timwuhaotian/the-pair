import type { TurnTokenUsage } from '../types'
import { isAcceptanceVerdictContent } from './acceptance'

export interface TokenUsageTurnCard {
  id: string
  role: 'mentor' | 'executor'
  state: 'live' | 'final'
  content: string
  activity: {
    phase: string
    label: string
    detail?: string
    startedAt: number
    updatedAt: number
  }
  startedAt: number
  updatedAt: number
  finalizedAt?: number
  tokenUsage?: TurnTokenUsage
}

export interface TokenUsageMessage {
  id: string
  timestamp: number
  from: 'mentor' | 'executor' | 'human'
  to: string
  type: string
  content: string
  iteration: number
  tokenUsage?: TurnTokenUsage
}

export function turnCardToMessage(
  card: TokenUsageTurnCard,
  iteration: number = 0
): TokenUsageMessage {
  const type =
    card.role === 'mentor'
      ? isAcceptanceVerdictContent(card.content)
        ? 'acceptance'
        : 'plan'
      : 'result'

  return {
    id: card.id,
    timestamp: card.updatedAt,
    from: card.role,
    to: 'human',
    type,
    content: card.content,
    iteration,
    tokenUsage: card.tokenUsage
  }
}

export function mergeTokenUsage(
  nextUsage: TurnTokenUsage | undefined,
  existingUsage: TurnTokenUsage | undefined
): TurnTokenUsage | undefined {
  return nextUsage ?? existingUsage
}

export function syncTokenUsage(
  backendValue: TurnTokenUsage | null | undefined,
  existingValue: TurnTokenUsage | undefined
): TurnTokenUsage | undefined {
  if (backendValue === null) {
    return undefined
  }
  if (backendValue !== undefined) {
    return backendValue
  }
  return existingValue
}

export function resolveCurrentTurnTokenUsage(
  backendValue: TurnTokenUsage | null | undefined,
  currentValue: TurnTokenUsage | undefined,
  fallbackValue: TurnTokenUsage | undefined
): TurnTokenUsage | undefined {
  if (backendValue === null) {
    return undefined
  }
  if (backendValue !== undefined) {
    return backendValue
  }
  return currentValue ?? fallbackValue
}

export function areTokenUsagesEqual(
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
