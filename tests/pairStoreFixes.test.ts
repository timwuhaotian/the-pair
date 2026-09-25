/**
 * Regression tests for usePairStore behavior around the backend IPC contract:
 * new-run archiving, handoff handling, deletion, plan review, Clear Session and
 * the global isLoading/error state. `window.api` is replaced by a scripted fake.
 */
import assert from 'node:assert/strict'
import test from 'node:test'

import {
  usePairStore,
  type AgentActivity,
  type Message,
  type Pair
} from '../src/renderer/src/store/usePairStore.ts'
import type { AvailableModel } from '../src/renderer/src/types.ts'

type Callback = (payload: unknown) => void

interface FakeApi {
  onMessage?: Callback
  onState?: Callback
  onHandoff?: Callback
  saved: Array<{ pairId: string; messages: unknown[] }>
  assignCalls: Array<{ pairId: string; input: { spec: string; role?: string } }>
  updateModelsCalls: Array<{ pairId: string; input: Record<string, unknown> }>
  pauseCalls: string[]
  interventions: Array<{ pairId: string; kind: string }>
  assignImpl: (pairId: string, input: { spec: string; role?: string }) => Promise<unknown>
  deleteImpl: (pairId: string) => Promise<unknown>
  backendState: Record<string, unknown> | null
  createImpl: (input: Record<string, unknown>) => Promise<unknown>
  createCalls: Array<Record<string, unknown>>
  refreshModels: () => Promise<unknown>
}

const api: FakeApi = {
  saved: [],
  assignCalls: [],
  updateModelsCalls: [],
  pauseCalls: [],
  interventions: [],
  assignImpl: async () => undefined,
  deleteImpl: async () => undefined,
  backendState: null,
  createImpl: async () => ({ pairId: 'created' }),
  createCalls: [],
  refreshModels: async () => []
}

Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: {
    api: {
      app: { restart: async () => undefined },
      pair: {
        create: async (input: Record<string, unknown>) => {
          api.createCalls.push(input)
          return api.createImpl(input)
        },
        assignTask: async (pairId: string, input: { spec: string; role?: string }) => {
          api.assignCalls.push({ pairId, input })
          return api.assignImpl(pairId, input)
        },
        updateModels: async (pairId: string, input: Record<string, unknown>) => {
          api.updateModelsCalls.push({ pairId, input })
          return input
        },
        pause: async (pairId: string) => {
          api.pauseCalls.push(pairId)
        },
        resume: async () => undefined,
        delete: async (pairId: string) => api.deleteImpl(pairId),
        retryTurn: async () => undefined,
        killProcess: async () => undefined,
        getState: async () => api.backendState,
        onMessage: async (callback: Callback) => {
          api.onMessage = callback
          return () => undefined
        },
        onState: async (callback: Callback) => {
          api.onState = callback
          return () => undefined
        },
        onHandoff: async (callback: Callback) => {
          api.onHandoff = callback
          return () => undefined
        }
      },
      session: {
        saveSnapshot: async (input: { pairId: string; messages: unknown[] }) => {
          api.saved.push(input)
        },
        loadAllPairs: async () => []
      },
      config: {
        getCachedModels: async () => [],
        refreshModels: () => api.refreshModels()
      },
      insights: {
        recordIntervention: async (input: { pairId: string; kind: string }) => {
          api.interventions.push(input)
        }
      }
    }
  }
})

// Silence expected error logs from the failure-path tests.
console.error = () => undefined
console.warn = () => undefined

usePairStore.getState().initMessageListener()

async function flush(): Promise<void> {
  for (let i = 0; i < 10; i += 1) {
    await new Promise((resolve) => setImmediate(resolve))
  }
}

const now = 1_000

function activity(label: string): AgentActivity {
  return { phase: 'idle', label, startedAt: now, updatedAt: now }
}

function message(id: string, from: Message['from'], timestamp: number, content = id): Message {
  return { id, from, to: 'human', type: 'result', content, timestamp, iteration: 1 }
}

function makePair(overrides: Partial<Pair> = {}): Pair {
  return {
    id: 'pair-1',
    name: 'Pair One',
    directory: '/tmp/repo',
    createdAt: now,
    status: 'Finished',
    iterations: 5,
    maxIterations: 10,
    cpuUsage: 0,
    memUsage: 0,
    spec: 'first task',
    mentorProvider: 'claude',
    mentorModel: 'claude/sonnet',
    executorProvider: 'codex',
    executorModel: 'codex/gpt-5.5',
    mentorReasoningEffort: 'high',
    executorReasoningEffort: 'low',
    messages: [message('m1', 'mentor', 2_000), message('e1', 'executor', 3_000)],
    mentorActivity: activity('Mentor idle'),
    executorActivity: activity('Executor idle'),
    mentorCpu: 0,
    mentorMemMb: 0,
    executorCpu: 0,
    executorMemMb: 0,
    modifiedFiles: [],
    gitTracking: { available: false },
    automationMode: 'full-auto',
    latestAcceptance: {
      iteration: 5,
      risk: 'low',
      checks: [],
      summary: 'all good',
      startedAt: 4_000,
      finishedAt: 5_000
    },
    turn: 'mentor',
    runCount: 1,
    runHistory: [],
    currentRunStartedAt: now,
    currentRunFinishedAt: 5_000,
    ...overrides
  }
}

const grokAlias: AvailableModel = {
  provider: 'grok',
  modelId: 'fast',
  displayName: 'fast',
  available: true,
  providerLabel: 'Grok Build',
  sourceProviderLabel: 'xAI',
  billingKind: 'byok',
  billingLabel: '',
  accessLabel: '',
  availabilityStatus: 'ready',
  supportsPairExecution: true,
  recommendedRoles: ['mentor', 'executor']
}

function reset(pairs: Pair[], extra: Record<string, unknown> = {}): void {
  api.saved = []
  api.assignCalls = []
  api.updateModelsCalls = []
  api.pauseCalls = []
  api.interventions = []
  api.assignImpl = async () => undefined
  api.deleteImpl = async () => undefined
  api.backendState = null
  api.createCalls = []
  usePairStore.setState({
    pairs,
    availableModels: [grokAlias],
    isLoading: false,
    error: null,
    modelsError: null,
    viewingRunId: null,
    restoringSpec: null,
    ...extra
  })
}

function getPair(id = 'pair-1'): Pair {
  const pair = usePairStore.getState().pairs.find((p) => p.id === id)
  assert.ok(pair, `pair ${id} exists`)
  return pair
}

function emitState(state: Record<string, unknown>): void {
  assert.ok(api.onState)
  api.onState({ pairId: 'pair-1', ...state })
}

// ── new run ────────────────────────────────────────────

test('a new run archives the run as it was before pair_assign_task, even when its pair:state races ahead', async () => {
  reset([makePair()])
  // The backend emits the new run's state from inside pair_assign_task.
  api.assignImpl = async () => {
    emitState({ status: 'mentoring', iteration: 1, turn: 'mentor', latestAcceptance: null })
  }

  await usePairStore.getState().assignTask('pair-1', 'second task', undefined, {
    executorModel: 'grok/fast'
  })

  const pair = getPair()
  assert.equal(pair.runHistory.length, 1)
  const archived = pair.runHistory[0]
  assert.equal(archived.status, 'Finished')
  assert.equal(archived.iterations, 5)
  assert.equal(archived.spec, 'first task')
  assert.equal(archived.finishedAt, 5_000)
  assert.equal(archived.latestAcceptance?.summary, 'all good')

  // Not forced back to Idle: the backend is already mentoring.
  assert.equal(pair.status, 'Mentoring')
  assert.equal(pair.iterations, 1)
  assert.equal(pair.spec, 'second task')
  assert.equal(pair.currentRunFinishedAt, undefined)
  assert.equal(pair.latestAcceptance, undefined)
  assert.deepEqual(
    pair.messages.map((m) => m.content),
    ['second task']
  )

  // Provider follows the override (a grok alias with no keyword), mentor untouched.
  assert.equal(pair.executorModel, 'grok/fast')
  assert.equal(pair.executorProvider, 'grok')
  assert.equal(pair.mentorProvider, 'claude')

  // Reasoning efforts were sent with the model update, not cleared.
  assert.equal(api.updateModelsCalls.length, 1)
  assert.equal(api.updateModelsCalls[0].input.mentorReasoningEffort, 'high')
  assert.equal(api.updateModelsCalls[0].input.executorReasoningEffort, 'low')
  assert.equal(api.updateModelsCalls[0].input.pendingExecutorModel, 'grok/fast')
})

test('a new run without a racing pair:state mirrors the mentor turn instead of Idle', async () => {
  reset([makePair()])
  await usePairStore.getState().assignTask('pair-1', 'second task')
  const pair = getPair()
  assert.equal(pair.status, 'Mentoring')
  assert.equal(pair.turn, 'mentor')
  assert.equal(pair.runHistory[0].status, 'Finished')
  assert.equal(pair.runCount, 2)
})

test('starting a new run leaves an archived-run view of that pair', async () => {
  reset([makePair({ runHistory: [] })])
  const archivedId = 'pair-1-run-1'
  await usePairStore.getState().assignTask('pair-1', 'second task')
  usePairStore.getState().setViewingRunId(archivedId)
  await usePairStore.getState().assignTask('pair-1', 'third task')
  assert.equal(usePairStore.getState().viewingRunId, null)
})

// ── currentRunFinishedAt ───────────────────────────────

test('currentRunFinishedAt is cleared on resume and re-stamped on the next pause', async () => {
  reset([makePair({ status: 'Executing', currentRunFinishedAt: undefined })])

  emitState({ status: 'paused', finishedAt: null })
  const firstPause = getPair().currentRunFinishedAt
  assert.equal(typeof firstPause, 'number')

  emitState({ status: 'executing', finishedAt: null })
  assert.equal(getPair().currentRunFinishedAt, undefined)

  await new Promise((resolve) => setTimeout(resolve, 5))
  emitState({ status: 'paused', finishedAt: null })
  const secondPause = getPair().currentRunFinishedAt
  assert.equal(typeof secondPause, 'number')
  assert.ok((secondPause as number) > (firstPause as number))

  emitState({ status: 'executing' })
  emitState({ status: 'finished', finishedAt: 9_999 })
  assert.equal(getPair().currentRunFinishedAt, 9_999)
})

test('a null currentRunFinishedAt (restored snapshot) still gets stamped when the run stops', () => {
  reset([
    makePair({
      status: 'Executing',
      currentRunFinishedAt: null as unknown as undefined
    })
  ])
  emitState({ status: 'error' })
  assert.equal(typeof getPair().currentRunFinishedAt, 'number')
})

// ── handoffs ───────────────────────────────────────────

test('a handoff for a pair the backend reports as paused (lowercase) is ignored', async () => {
  reset([makePair({ status: 'Executing' })])
  api.backendState = { status: 'paused', messages: [] }
  assert.ok(api.onHandoff)
  api.onHandoff({ pairId: 'pair-1', nextRole: 'mentor' })
  await flush()
  assert.equal(api.assignCalls.length, 0)
})

test('background handoffs never touch the global isLoading/error', async () => {
  reset([makePair({ status: 'Executing' })], { error: 'shown in a modal' })
  api.backendState = { status: 'executing', messages: [] }
  let release: () => void = () => undefined
  let sawLoading: boolean | undefined
  api.assignImpl = () =>
    new Promise<void>((resolve) => {
      sawLoading = usePairStore.getState().isLoading
      release = resolve
    })

  assert.ok(api.onHandoff)
  api.onHandoff({ pairId: 'pair-1', nextRole: 'mentor' })
  await flush()
  assert.equal(api.assignCalls.length, 1)
  assert.equal(api.assignCalls[0].input.role, 'mentor')
  assert.equal(sawLoading, false)
  release()
  await flush()
  assert.equal(usePairStore.getState().isLoading, false)
  assert.equal(usePairStore.getState().error, 'shown in a modal')
})

test('a HANDOFF_IGNORED rejection is dropped silently', async () => {
  reset([makePair({ status: 'Executing' })])
  api.backendState = { status: 'executing', messages: [] }
  api.assignImpl = async () => {
    throw 'HANDOFF_IGNORED: pair is paused'
  }
  assert.ok(api.onHandoff)
  api.onHandoff({ pairId: 'pair-1', nextRole: 'executor' })
  await flush()
  assert.equal(api.assignCalls.length, 1)
  assert.equal(api.pauseCalls.length, 0)
  assert.equal(usePairStore.getState().error, null)
  assert.equal(getPair().handoffError, undefined)
})

test('a failed handoff is recorded on the pair (not the global error) and pauses it', async () => {
  reset([makePair({ status: 'Executing' })])
  api.backendState = { status: 'executing', messages: [] }
  api.assignImpl = async () => {
    throw 'spawn failed'
  }
  assert.ok(api.onHandoff)
  api.onHandoff({ pairId: 'pair-1', nextRole: 'executor' })
  await flush()
  assert.deepEqual(api.pauseCalls, ['pair-1'])
  assert.equal(usePairStore.getState().error, null)
  assert.equal(getPair().handoffError, 'Handoff failed: spawn failed')
})

// ── deletion ───────────────────────────────────────────

test('events during pair_delete do not write a snapshot that resurrects the pair', async () => {
  reset([makePair({ status: 'Executing' })])
  api.deleteImpl = async () => {
    // Killing the processes emits state + messages while the delete runs.
    emitState({ status: 'paused' })
    assert.ok(api.onMessage)
    api.onMessage({ pairId: 'pair-1', message: message('late', 'executor', 9_000) })
  }
  await usePairStore.getState().deletePair('pair-1')
  assert.equal(api.saved.length, 0)
  assert.equal(usePairStore.getState().pairs.length, 0)
})

test('a failed delete keeps the pair and resumes saving its snapshot', async () => {
  reset([makePair({ id: 'pair-2', status: 'Executing' })])
  api.deleteImpl = async () => {
    throw 'worktree has unsaved changes'
  }
  await assert.rejects(usePairStore.getState().deletePair('pair-2'))
  assert.equal(usePairStore.getState().pairs.length, 1)
  await usePairStore.getState().flushSnapshots()
  assert.deepEqual(
    api.saved.map((s) => s.pairId),
    ['pair-2']
  )
})

// ── plan review ────────────────────────────────────────

test('plan review records the decision only after the handoff succeeds', async () => {
  reset([makePair({ status: 'Awaiting Human Review', messages: [] })])
  api.assignImpl = async () => {
    throw 'spawn failed'
  }
  await assert.rejects(usePairStore.getState().resolvePlanReview('pair-1', 'reject', 'too big'))
  assert.equal(getPair().messages.length, 0)
  assert.equal(api.interventions.length, 0)
  assert.equal(usePairStore.getState().error, null)

  // Retry succeeds: exactly one decision message and one intervention.
  api.assignImpl = async () => undefined
  await usePairStore.getState().resolvePlanReview('pair-1', 'reject', 'too big')
  assert.deepEqual(
    getPair().messages.map((m) => [m.from, m.type, m.content]),
    [['human', 'feedback', 'too big']]
  )
  assert.deepEqual(
    api.interventions.map((i) => i.kind),
    ['plan_rejected']
  )
  assert.equal(api.assignCalls.at(-1)?.input.role, 'mentor')
})

test('plan approval from Awaiting Human Review hands off to the executor', async () => {
  reset([makePair({ status: 'Awaiting Human Review', messages: [] })])
  await usePairStore.getState().resolvePlanReview('pair-1', 'approve')
  assert.equal(api.assignCalls.length, 1)
  assert.equal(api.assignCalls[0].input.role, 'executor')
  assert.deepEqual(
    api.interventions.map((i) => i.kind),
    ['plan_approved']
  )
})

// ── Clear Session ──────────────────────────────────────

test('Clear Session sticks when the backend re-sends the old transcript', () => {
  reset([makePair({ status: 'Idle' })])
  const old = getPair().messages
  usePairStore.getState().setMessages('pair-1', [])
  assert.equal(getPair().messages.length, 0)

  emitState({ status: 'paused', messages: old })
  assert.equal(getPair().messages.length, 0)

  const fresh = message('new', 'mentor', Date.now() + 10_000)
  emitState({ status: 'paused', messages: [...old, fresh] })
  assert.deepEqual(
    getPair().messages.map((m) => m.id),
    ['new']
  )
})

// ── createPair / models ────────────────────────────────

test('createPair keeps the reasoning efforts chosen at creation and stores codex ids qualified', async () => {
  reset([])
  const codex: AvailableModel = { ...grokAlias, provider: 'codex', modelId: 'codex-mini-latest' }
  usePairStore.setState({ availableModels: [grokAlias, codex] })
  api.createImpl = async () => ({ pairId: 'created' })

  await usePairStore.getState().createPair({
    name: 'New',
    directory: '/tmp/repo',
    spec: 'task',
    mentorModel: 'codex/codex-mini-latest',
    executorModel: 'grok/fast',
    mentorReasoningEffort: 'high',
    executorReasoningEffort: 'medium'
  })

  const pair = getPair('created')
  assert.equal(pair.mentorReasoningEffort, 'high')
  assert.equal(pair.executorReasoningEffort, 'medium')
  const mentor = api.createCalls[0].mentor as { model: string; provider: string }
  const executor = api.createCalls[0].executor as { model: string; provider: string }
  assert.deepEqual(mentor, { role: 'mentor', provider: 'codex', model: 'codex/codex-mini-latest' })
  assert.deepEqual(executor, { role: 'executor', provider: 'grok', model: 'grok/fast' })
})

test('a background model refresh failure does not set the global modal error', async () => {
  reset([])
  api.refreshModels = async () => {
    throw new Error('offline')
  }
  await usePairStore.getState().loadAvailableModels()
  assert.equal(usePairStore.getState().error, null)
  assert.equal(usePairStore.getState().modelsError, 'Failed to load models')
})
