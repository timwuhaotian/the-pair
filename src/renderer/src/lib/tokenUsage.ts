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
  cognitiveEvents?: unknown[]
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
  cognitiveEvents?: unknown[]
  startedAt?: number
  finalizedAt?: number
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
    tokenUsage: card.tokenUsage,
    cognitiveEvents: card.cognitiveEvents,
    startedAt: card.startedAt,
    finalizedAt: card.finalizedAt ?? card.updatedAt
  }
}

/** Field-level merge: backend `null` clears (object gone → undefined; a field
 *  explicitly null → that field's cleared value), `undefined` (absent) keeps.
 *  Serde may still emit `inputTokens: null` for unreported optionals; TS types
 *  mark it optional, so a bare null clears an optional field when an earlier
 *  live event had supplied it. */
export function mergeTokenUsage(
  nextUsage: TurnTokenUsage | null | undefined,
  existingUsage: TurnTokenUsage | undefined
): TurnTokenUsage | undefined {
  if (nextUsage === undefined) return existingUsage
  if (nextUsage === null) return undefined
  const next = nextUsage as unknown as Record<string, unknown>
  if (!existingUsage && next['outputTokens'] === null) return undefined

  const nextOutput = next['outputTokens']
  const nextInput = next['inputTokens']
  const nextTimestamp = next['lastUpdatedAt']
  const outputTokens =
    nextOutput === null
      ? 0
      : typeof nextOutput === 'number' && Number.isFinite(nextOutput)
        ? nextOutput
        : (existingUsage?.outputTokens ?? 0)
  const inputTokens =
    nextInput === null
      ? undefined
      : typeof nextInput === 'number' && Number.isFinite(nextInput)
        ? nextInput
        : existingUsage?.inputTokens
  const lastUpdatedAt =
    typeof nextTimestamp === 'number' && Number.isFinite(nextTimestamp)
      ? nextTimestamp
      : (existingUsage?.lastUpdatedAt ?? Date.now())
  const nextSource = next['source']
  const source = (
    typeof nextSource === 'string' ? nextSource : (existingUsage?.source ?? 'live')
  ) as TurnTokenUsage['source']
  const nextProvider = next['provider']
  const provider =
    nextProvider === null
      ? undefined
      : typeof nextProvider === 'string'
        ? nextProvider
        : existingUsage?.provider

  return { outputTokens, inputTokens, lastUpdatedAt, source, provider }
}

export function syncTokenUsage(
  backendValue: TurnTokenUsage | null | undefined,
  existingValue: TurnTokenUsage | undefined
): TurnTokenUsage | undefined {
  return mergeTokenUsage(backendValue, existingValue)
}

export function resolveCurrentTurnTokenUsage(
  backendValue: TurnTokenUsage | null | undefined,
  currentValue: TurnTokenUsage | undefined,
  fallbackValue: TurnTokenUsage | undefined
): TurnTokenUsage | undefined {
  if (backendValue === null) {
    return undefined
  }
  if (backendValue === undefined) {
    return currentValue ?? fallbackValue
  }
  return mergeTokenUsage(backendValue, mergeTokenUsage(currentValue, fallbackValue))
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
