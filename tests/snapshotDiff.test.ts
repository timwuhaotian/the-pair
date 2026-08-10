import assert from 'node:assert/strict'
import test from 'node:test'

import {
  shouldSaveSnapshot,
  type SnapshotComparablePair
} from '../src/renderer/src/lib/snapshotDiff.ts'
import type { TurnTokenUsage } from '../src/renderer/src/types.ts'

function makeTokenUsage(outputTokens: number): TurnTokenUsage {
  return {
    outputTokens,
    inputTokens: 100,
    lastUpdatedAt: 2000,
    source: 'live',
    provider: 'claude'
  }
}

function makePair(): SnapshotComparablePair {
  return {
    status: 'Mentoring',
    turn: 'mentor',
    iterations: 1,
    currentRunStartedAt: 1000,
    runCount: 1,
    mentorModel: 'claude/sonnet',
    executorModel: 'gpt-4o-mini',
    currentTurnCard: {
      content: 'Mentor working',
      state: 'live',
      tokenUsage: makeTokenUsage(10)
    }
  }
}

test('shouldSaveSnapshot ignores unchanged pairs', () => {
  const previous = makePair()
  const next = makePair()

  assert.equal(shouldSaveSnapshot(previous, next), false)
})

test('shouldSaveSnapshot persists token usage changes on the active turn card', () => {
  const previous = makePair()
  const next = makePair()
  next.currentTurnCard = {
    ...next.currentTurnCard!,
    tokenUsage: makeTokenUsage(42)
  }

  assert.equal(shouldSaveSnapshot(previous, next), true)
})

test('shouldSaveSnapshot persists turn-card content changes', () => {
  const previous = makePair()
  const next = makePair()
  next.currentTurnCard = {
    ...next.currentTurnCard!,
    content: 'Mentor has new instructions'
  }

  assert.equal(shouldSaveSnapshot(previous, next), true)
})

test('shouldSaveSnapshot persists status change', () => {
  const previous = makePair()
  const next = makePair()
  next.status = 'Executing'
  assert.equal(shouldSaveSnapshot(previous, next), true)
})

test('shouldSaveSnapshot persists turn change', () => {
  const previous = makePair()
  const next = makePair()
  next.turn = 'executor'
  assert.equal(shouldSaveSnapshot(previous, next), true)
})

test('shouldSaveSnapshot persists iterations change', () => {
  const previous = makePair()
  const next = makePair()
  next.iterations = 2
  assert.equal(shouldSaveSnapshot(previous, next), true)
})

test('shouldSaveSnapshot persists currentRunFinishedAt change', () => {
  const previous = makePair()
  const next = makePair()
  next.currentRunFinishedAt = 5000
  assert.equal(shouldSaveSnapshot(previous, next), true)
})

test('shouldSaveSnapshot persists runCount change', () => {
  const previous = makePair()
  const next = makePair()
  next.runCount = 2
  assert.equal(shouldSaveSnapshot(previous, next), true)
})

test('shouldSaveSnapshot persists pendingMentorModel change', () => {
  const previous = makePair()
  const next = makePair()
  next.pendingMentorModel = 'claude/sonnet'
  assert.equal(shouldSaveSnapshot(previous, next), true)
})

test('shouldSaveSnapshot persists pendingExecutorModel change', () => {
  const previous = makePair()
  const next = makePair()
  next.pendingExecutorModel = 'gpt-4o'
  assert.equal(shouldSaveSnapshot(previous, next), true)
})

test('shouldSaveSnapshot persists mentorModel change', () => {
  const previous = makePair()
  const next = makePair()
  next.mentorModel = 'claude/opus'
  assert.equal(shouldSaveSnapshot(previous, next), true)
})

test('shouldSaveSnapshot persists executorModel change', () => {
  const previous = makePair()
  const next = makePair()
  next.executorModel = 'gpt-4o'
  assert.equal(shouldSaveSnapshot(previous, next), true)
})

test('shouldSaveSnapshot persists turn-card state change', () => {
  const previous = makePair()
  const next = makePair()
  next.currentTurnCard = {
    ...next.currentTurnCard!,
    state: 'final'
  }
  assert.equal(shouldSaveSnapshot(previous, next), true)
})
