import assert from 'node:assert/strict'
import test from 'node:test'

import {
  buildTimelineSource,
  isNearBottom,
  resolveViewingRun,
  selectVisibleTurnCard,
  splitComposition
} from '../src/renderer/src/components/consoleView.ts'
import type { Message, PairRunSummary, TurnCard } from '../src/renderer/src/store/usePairStore.ts'

function message(overrides: Partial<Message>): Message {
  return {
    id: 'm',
    timestamp: 1,
    from: 'mentor',
    to: 'executor',
    type: 'plan',
    content: 'x',
    iteration: 1,
    ...overrides
  }
}

function run(overrides: Partial<PairRunSummary> = {}): PairRunSummary {
  return {
    id: 'run-1',
    spec: 'old task',
    status: 'Finished',
    startedAt: 1_000,
    finishedAt: 5_000,
    mentorModel: 'old-mentor',
    executorModel: 'old-executor',
    iterations: 3,
    messages: [message({ id: 'a', timestamp: 1_500 }), message({ id: 'b', timestamp: 4_500 })],
    ...overrides
  }
}

function card(role: TurnCard['role']): TurnCard {
  return {
    id: `card-${role}`,
    role,
    state: 'live',
    content: '',
    activity: { phase: 'thinking', label: 'Thinking', detail: null, updatedAt: 0 },
    startedAt: 0,
    updatedAt: 0,
    cognitiveEvents: []
  } as unknown as TurnCard
}

test('a viewingRunId that is not in this pair’s history is treated as live', () => {
  const history = [run()]
  assert.equal(resolveViewingRun(history, 'run-1')?.id, 'run-1')
  assert.equal(resolveViewingRun(history, 'run-from-another-pair'), null)
  assert.equal(resolveViewingRun([], 'run-1'), null)
  assert.equal(resolveViewingRun(history, null), null)
})

test('isNearBottom uses the stick-to-bottom threshold', () => {
  assert.equal(isNearBottom({ scrollHeight: 1000, scrollTop: 600, clientHeight: 300 }), true)
  assert.equal(isNearBottom({ scrollHeight: 1000, scrollTop: 100, clientHeight: 300 }), false)
})

test('the live turn card is compared against the unfiltered transcript', () => {
  const live = [message({ from: 'human' }), message({ from: 'executor' })]
  // The mentor is live after an executor message — shown even if a filter hides executor output.
  assert.equal(selectVisibleTurnCard(live, card('mentor'))?.role, 'mentor')
  // Once the mentor's own final message lands, the card hides.
  assert.equal(selectVisibleTurnCard([...live, message({ from: 'mentor' })], card('mentor')), null)
  assert.equal(selectVisibleTurnCard([], card('executor'))?.role, 'executor')
  assert.equal(selectVisibleTurnCard(live, undefined), null)
})

test('IME composing text is underlined as a slice and never rendered twice', () => {
  // textarea value already contains the composing text (normal WebKit/Chrome case)
  assert.deepEqual(splitComposition('fix にほん later', 'にほん', 4), {
    before: 'fix ',
    composing: 'にほん',
    after: ' later'
  })
  // value does not contain it yet → inserted once at the composition start
  assert.deepEqual(splitComposition('fix  later', 'にほん', 4), {
    before: 'fix ',
    composing: 'にほん',
    after: ' later'
  })
  assert.deepEqual(splitComposition('plain', '', null), {
    before: 'plain',
    composing: '',
    after: ''
  })
})

const livePair = {
  name: 'pair',
  spec: 'live task',
  mentorModel: 'live-mentor',
  executorModel: 'live-executor',
  status: 'Executing' as const,
  messages: [message({ id: 'live' })],
  latestAcceptance: undefined,
  modifiedFiles: [{ path: 'src/live.ts', status: 'M', displayPath: 'src/live.ts' }],
  currentRunStartedAt: 9_000,
  currentRunFinishedAt: undefined
}

test('reports for a past run use that run’s own dates, status and models', () => {
  const source = buildTimelineSource(livePair, run())
  assert.equal(source.spec, 'old task')
  assert.equal(source.mentorModel, 'old-mentor')
  assert.equal(source.status, 'Finished')
  assert.equal(source.currentRunStartedAt, 1_000)
  assert.equal(source.currentRunFinishedAt, 5_000)
  // Runs don't record their file list — never report the live run's files.
  assert.deepEqual(source.modifiedFiles, [])
})

test('a past run without finishedAt ends at its last message, not "now"', () => {
  const source = buildTimelineSource(livePair, run({ finishedAt: undefined }))
  assert.equal(source.currentRunFinishedAt, 4_500)
})

test('the live timeline source uses the live pair', () => {
  const source = buildTimelineSource(livePair, null)
  assert.equal(source.spec, 'live task')
  assert.equal(source.currentRunStartedAt, 9_000)
  assert.equal(source.modifiedFiles.length, 1)
})
