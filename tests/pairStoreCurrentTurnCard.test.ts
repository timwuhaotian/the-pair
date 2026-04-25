import assert from 'node:assert/strict'
import test from 'node:test'

import {
  usePairStore,
  type Pair,
  type Message,
  type AgentActivity
} from '../src/renderer/src/store/usePairStore.ts'

const now = 1_000
let onMessage: ((payload: unknown) => void) | undefined

function activity(phase: AgentActivity['phase'], label: string, detail?: string): AgentActivity {
  return {
    phase,
    label,
    detail,
    startedAt: now,
    updatedAt: now
  }
}

function message(
  id: string,
  from: Message['from'],
  type: Message['type'],
  content: string
): Message {
  return {
    id,
    from,
    type,
    content,
    timestamp: now,
    to: 'human',
    iteration: 2
  }
}

function makePair(messages: Message[]): Pair {
  return {
    id: 'pair-current-card',
    name: 'Current Card Pair',
    directory: '/tmp/the-pair',
    createdAt: now,
    status: 'Reviewing',
    iterations: 2,
    maxIterations: 10,
    cpuUsage: 0,
    memUsage: 0,
    spec: 'test task',
    mentorProvider: 'claude',
    mentorModel: 'claude/sonnet',
    executorProvider: 'codex',
    executorModel: 'codex/gpt',
    messages,
    mentorActivity: activity('idle', 'Mentor idle'),
    executorActivity: activity('idle', 'Executor idle'),
    mentorCpu: 0,
    mentorMemMb: 0,
    executorCpu: 0,
    executorMemMb: 0,
    modifiedFiles: [],
    gitTracking: { available: false },
    automationMode: 'full-auto',
    turn: 'mentor',
    runCount: 1,
    runHistory: [],
    currentRunStartedAt: now
  }
}

test('syncFullState shows a new mentor live card after a prior mentor acceptance', () => {
  const messages = [
    message('mentor-acceptance', 'mentor', 'acceptance', '{"verdict":"fail"}'),
    message('executor-result', 'executor', 'result', 'Conversation test complete.')
  ]

  usePairStore.setState({
    pairs: [makePair(messages)],
    availableModels: [],
    isLoading: false,
    error: null,
    viewingRunId: null,
    restoringSpec: null
  })

  usePairStore.getState().syncFullState('pair-current-card', {
    status: 'Reviewing',
    iteration: 3,
    turn: 'mentor',
    messages,
    mentorActivity: activity('thinking', 'Mentor active', 'Thinking...'),
    executorActivity: activity('waiting', 'Executor standing by')
  })

  const pair = usePairStore.getState().pairs[0]

  assert.equal(pair.currentTurnCard?.role, 'mentor')
  assert.equal(pair.currentTurnCard?.state, 'live')
  assert.equal(pair.currentTurnCard?.content, 'Thinking...')
})

test('message events do not archive progress-only cards as real console messages', () => {
  Object.defineProperty(globalThis, 'window', {
    value: {
      api: {
        pair: {
          onMessage: (callback: (payload: unknown) => void) => {
            onMessage = callback
          },
          onState: () => {},
          onHandoff: () => {},
          getState: async () => null
        },
        session: {
          saveSnapshot: async () => {}
        }
      }
    },
    configurable: true
  })

  usePairStore.setState({
    pairs: [makePair([])],
    availableModels: [],
    isLoading: false,
    error: null,
    viewingRunId: null,
    restoringSpec: null
  })

  usePairStore.getState().initMessageListener()
  assert.ok(onMessage, 'message listener should be registered')

  onMessage({
    pairId: 'pair-current-card',
    message: message('mentor-progress', 'mentor', 'progress', 'Starting process...')
  })
  onMessage({
    pairId: 'pair-current-card',
    message: message('mentor-final', 'mentor', 'plan', 'Send a short hello.')
  })

  const pair = usePairStore.getState().pairs[0]

  assert.deepEqual(
    pair.messages.map((entry) => entry.id),
    ['turn-mentor-iter2', 'mentor-final']
  )
  assert.equal(pair.messages[0].content, 'Starting process...')
  assert.equal(pair.messages[1].content, 'Send a short hello.')
  assert.equal(pair.currentTurnCard, undefined)
})

test('switching roles clears progress-only cards instead of archiving them', () => {
  Object.defineProperty(globalThis, 'window', {
    value: {
      api: {
        pair: {
          onMessage: (callback: (payload: unknown) => void) => {
            onMessage = callback
          },
          onState: () => {},
          onHandoff: () => {},
          getState: async () => null
        },
        session: {
          saveSnapshot: async () => {}
        }
      }
    },
    configurable: true
  })

  usePairStore.setState({
    pairs: [makePair([])],
    availableModels: [],
    isLoading: false,
    error: null,
    viewingRunId: null,
    restoringSpec: null
  })

  usePairStore.getState().initMessageListener()
  assert.ok(onMessage, 'message listener should be registered')

  onMessage({
    pairId: 'pair-current-card',
    message: message('mentor-progress', 'mentor', 'progress', 'Starting process...')
  })
  onMessage({
    pairId: 'pair-current-card',
    message: message('executor-progress', 'executor', 'progress', 'Executor standing by')
  })

  const pair = usePairStore.getState().pairs[0]

  assert.equal(pair.messages.length, 1)
  assert.equal(pair.messages[0].from, 'mentor')
  assert.equal(pair.messages[0].content, 'Starting process...')
  assert.equal(pair.currentTurnCard?.role, 'executor')
  assert.equal(pair.currentTurnCard?.content, 'Executor standing by')
})
