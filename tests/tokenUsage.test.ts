import assert from 'node:assert/strict'
import test from 'node:test'

import type { TurnTokenUsage } from '../src/renderer/src/types.ts'
import {
  turnCardToMessage,
  mergeTokenUsage,
  syncTokenUsage,
  resolveCurrentTurnTokenUsage,
  areTokenUsagesEqual,
  type TokenUsageTurnCard
} from '../src/renderer/src/lib/tokenUsage.ts'

test('turnCardToMessage preserves tokenUsage when archiving turn card', () => {
  const card: TokenUsageTurnCard = {
    id: 'turn-123',
    role: 'mentor',
    state: 'final',
    content: 'Analysis complete',
    activity: {
      phase: 'idle',
      label: 'Done',
      startedAt: 1000,
      updatedAt: 2000
    },
    startedAt: 1000,
    updatedAt: 2000,
    finalizedAt: 2000,
    tokenUsage: {
      outputTokens: 1234,
      inputTokens: 500,
      lastUpdatedAt: 2000,
      source: 'final',
      provider: 'claude'
    }
  }

  const message = turnCardToMessage(card)

  assert.equal(message.id, 'turn-123')
  assert.equal(message.from, 'mentor')
  assert.equal(message.content, 'Analysis complete')
  assert.ok(message.tokenUsage, 'tokenUsage should be preserved')
  assert.equal(message.tokenUsage?.outputTokens, 1234)
  assert.equal(message.tokenUsage?.inputTokens, 500)
  assert.equal(message.tokenUsage?.source, 'final')
  assert.equal(message.tokenUsage?.provider, 'claude')
})

test('turnCardToMessage handles missing tokenUsage gracefully', () => {
  const card: TokenUsageTurnCard = {
    id: 'turn-456',
    role: 'executor',
    state: 'final',
    content: 'Execution done',
    activity: {
      phase: 'idle',
      label: 'Done',
      startedAt: 1000,
      updatedAt: 2000
    },
    startedAt: 1000,
    updatedAt: 2000,
    finalizedAt: 2000
  }

  const message = turnCardToMessage(card)

  assert.equal(message.id, 'turn-456')
  assert.equal(message.tokenUsage, undefined)
})

test('turnCardToMessage preserves live tokenUsage for in-progress turns', () => {
  const card: TokenUsageTurnCard = {
    id: 'turn-789',
    role: 'executor',
    state: 'live',
    content: 'Processing...',
    activity: {
      phase: 'responding',
      label: 'Working',
      startedAt: 1000,
      updatedAt: 2000
    },
    startedAt: 1000,
    updatedAt: 2000,
    tokenUsage: {
      outputTokens: 500,
      lastUpdatedAt: 2000,
      source: 'live',
      provider: 'opencode'
    }
  }

  const message = turnCardToMessage(card)

  assert.ok(message.tokenUsage, 'live tokenUsage should be preserved')
  assert.equal(message.tokenUsage?.outputTokens, 500)
  assert.equal(message.tokenUsage?.source, 'live')
})

test('turnCardToMessage archives mentor acceptance cards as acceptance messages', () => {
  const card: TokenUsageTurnCard = {
    id: 'turn-acceptance',
    role: 'mentor',
    state: 'final',
    content: `{
      "verdict": "fail",
      "risk": "medium",
      "evidence": ["Button alignment change is not visible"],
      "summary": "The requested UI update is incomplete",
      "nextStep": {
        "action": "continue",
        "instructions": ["Left-align the Step 3 action buttons", "Re-run verification"]
      }
    }`,
    activity: {
      phase: 'idle',
      label: 'Done',
      startedAt: 1000,
      updatedAt: 2000
    },
    startedAt: 1000,
    updatedAt: 2000,
    finalizedAt: 2000
  }

  const message = turnCardToMessage(card)

  assert.equal(message.type, 'acceptance')
})

test('mergeTokenUsage preserves existing when next is undefined', () => {
  const existingUsage: TurnTokenUsage = {
    outputTokens: 2500,
    inputTokens: 800,
    lastUpdatedAt: 3000,
    source: 'final',
    provider: 'gemini'
  }

  const result = mergeTokenUsage(undefined, existingUsage)

  assert.ok(result, 'existing tokenUsage should be preserved')
  assert.equal(result?.outputTokens, 2500)
  assert.equal(result?.source, 'final')
  assert.equal(result?.provider, 'gemini')
})

test('mergeTokenUsage prefers next when both are defined', () => {
  const existingUsage: TurnTokenUsage = {
    outputTokens: 100,
    lastUpdatedAt: 2000,
    source: 'live',
    provider: 'claude'
  }

  const nextUsage: TurnTokenUsage = {
    outputTokens: 500,
    lastUpdatedAt: 4000,
    source: 'live',
    provider: 'claude'
  }

  const result = mergeTokenUsage(nextUsage, existingUsage)

  assert.ok(result)
  assert.equal(result?.outputTokens, 500)
  assert.equal(result?.lastUpdatedAt, 4000)
})

test('mergeTokenUsage returns undefined when both are undefined', () => {
  const result = mergeTokenUsage(undefined, undefined)
  assert.equal(result, undefined)
})

test('mergeTokenUsage returns next when existing is undefined', () => {
  const nextUsage: TurnTokenUsage = {
    outputTokens: 750,
    lastUpdatedAt: 5000,
    source: 'final',
    provider: 'opencode'
  }

  const result = mergeTokenUsage(nextUsage, undefined)

  assert.ok(result)
  assert.equal(result?.outputTokens, 750)
  assert.equal(result?.source, 'final')
})

test('syncTokenUsage clears value when backend sends null', () => {
  const existingUsage: TurnTokenUsage = {
    outputTokens: 1000,
    lastUpdatedAt: 2000,
    source: 'live',
    provider: 'claude'
  }

  const result = syncTokenUsage(null, existingUsage)

  assert.equal(result, undefined, 'null from backend should clear existing value')
})

test('syncTokenUsage uses backend value when provided', () => {
  const existingUsage: TurnTokenUsage = {
    outputTokens: 100,
    lastUpdatedAt: 1000,
    source: 'live',
    provider: 'claude'
  }

  const backendValue: TurnTokenUsage = {
    outputTokens: 500,
    lastUpdatedAt: 2000,
    source: 'final',
    provider: 'claude'
  }

  const result = syncTokenUsage(backendValue, existingUsage)

  assert.ok(result)
  assert.equal(result?.outputTokens, 500)
  assert.equal(result?.source, 'final')
})

test('syncTokenUsage keeps existing when backend sends undefined', () => {
  const existingUsage: TurnTokenUsage = {
    outputTokens: 750,
    lastUpdatedAt: 3000,
    source: 'live',
    provider: 'opencode'
  }

  const result = syncTokenUsage(undefined, existingUsage)

  assert.ok(result, 'undefined from backend should keep existing value')
  assert.equal(result?.outputTokens, 750)
  assert.equal(result?.source, 'live')
})

test('syncTokenUsage returns undefined when both are undefined/null', () => {
  assert.equal(syncTokenUsage(null, undefined), undefined)
  assert.equal(syncTokenUsage(undefined, undefined), undefined)
})

test('resolveCurrentTurnTokenUsage clears stale usage when backend resets the turn', () => {
  const existingUsage: TurnTokenUsage = {
    outputTokens: 900,
    lastUpdatedAt: 2000,
    source: 'final',
    provider: 'claude'
  }

  assert.equal(
    resolveCurrentTurnTokenUsage(null, existingUsage, existingUsage),
    undefined,
    'null from backend should clear the live token chip'
  )
})

test('resolveCurrentTurnTokenUsage preserves the current turn usage when backend is silent', () => {
  const currentUsage: TurnTokenUsage = {
    outputTokens: 111,
    lastUpdatedAt: 1000,
    source: 'live',
    provider: 'opencode'
  }
  const fallbackUsage: TurnTokenUsage = {
    outputTokens: 222,
    lastUpdatedAt: 900,
    source: 'final',
    provider: 'opencode'
  }

  const resolved = resolveCurrentTurnTokenUsage(undefined, currentUsage, fallbackUsage)
  assert.equal(resolved, currentUsage)
})

test('mergeTokenUsage merges fields and honors explicit null clears', () => {
  const existing: TurnTokenUsage = {
    outputTokens: 10,
    inputTokens: 100,
    lastUpdatedAt: 1000,
    source: 'live',
    provider: 'primary'
  }
  const partial = mergeTokenUsage(
    { outputTokens: 20, lastUpdatedAt: 2000, source: 'final' },
    existing
  )
  assert.equal(partial?.inputTokens, 100)
  assert.equal(partial?.provider, 'primary')

  const cleared = mergeTokenUsage(
    {
      outputTokens: 20,
      inputTokens: null,
      lastUpdatedAt: 2000,
      source: 'final',
      provider: null
    } as unknown as TurnTokenUsage,
    existing
  )
  assert.equal(cleared?.inputTokens, undefined)
  assert.equal(cleared?.provider, undefined)
})

test('mergeTokenUsage drops an empty live usage payload', () => {
  assert.equal(
    mergeTokenUsage({ outputTokens: null } as unknown as TurnTokenUsage, undefined),
    undefined
  )
})

test('mergeTokenUsage handles cleared numbers and live defaults', () => {
  const existing: TurnTokenUsage = {
    outputTokens: 10,
    inputTokens: 100,
    lastUpdatedAt: 1000,
    source: 'live'
  }
  const cleared = mergeTokenUsage({ outputTokens: null } as unknown as TurnTokenUsage, existing)
  assert.equal(cleared?.outputTokens, 0)
  assert.equal(cleared?.lastUpdatedAt, 1000)

  const fresh = mergeTokenUsage({} as TurnTokenUsage, undefined)
  assert.equal(fresh?.outputTokens, 0)
  assert.equal(fresh?.source, 'live')
  assert.ok(fresh && fresh.lastUpdatedAt > 0)
})

test('resolveCurrentTurnTokenUsage merges backend fields over the live fallback', () => {
  const current: TurnTokenUsage = {
    outputTokens: 10,
    inputTokens: 100,
    lastUpdatedAt: 1000,
    source: 'live',
    provider: 'primary'
  }
  const resolved = resolveCurrentTurnTokenUsage(
    { outputTokens: 20, lastUpdatedAt: 2000, source: 'final' },
    current,
    undefined
  )
  assert.equal(resolved?.inputTokens, 100)
  assert.equal(resolved?.provider, 'primary')
  assert.equal(resolved?.outputTokens, 20)
})

test('areTokenUsagesEqual compares the token usage payload structurally', () => {
  const left: TurnTokenUsage = {
    outputTokens: 50,
    inputTokens: 10,
    lastUpdatedAt: 1234,
    source: 'live',
    provider: 'gemini'
  }
  const right: TurnTokenUsage = {
    outputTokens: 50,
    inputTokens: 10,
    lastUpdatedAt: 1234,
    source: 'live',
    provider: 'gemini'
  }
  const different: TurnTokenUsage = {
    outputTokens: 51,
    inputTokens: 10,
    lastUpdatedAt: 1234,
    source: 'live',
    provider: 'gemini'
  }

  assert.equal(areTokenUsagesEqual(left, right), true)
  assert.equal(areTokenUsagesEqual(left, different), false)
  assert.equal(areTokenUsagesEqual(undefined, undefined), true)
})
