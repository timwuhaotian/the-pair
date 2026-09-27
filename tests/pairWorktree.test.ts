/**
 * Tests for the worktree-per-task flow:
 *   - createPair forwards the worktree opt-in and stores the workspace the
 *     backend reports back (worktree path + the-pair/… branch)
 *   - assignTask forwards the fresh-worktree request for a new run and syncs
 *     the (possibly rotated) workspace from the backend's response
 *   - a plain new run sends no worktree field and keeps the workspace as-is
 *
 * `window.api` is replaced by a scripted fake (same harness as
 * pairStoreFixes.test.ts).
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
  onState?: Callback
  saved: Array<Record<string, unknown>>
  assignCalls: Array<{ pairId: string; input: Record<string, unknown> }>
  createCalls: Array<Record<string, unknown>>
  assignImpl: (pairId: string, input: Record<string, unknown>) => Promise<unknown>
  createImpl: (input: Record<string, unknown>) => Promise<unknown>
}

const api: FakeApi = {
  saved: [],
  assignCalls: [],
  createCalls: [],
  assignImpl: async () => undefined,
  createImpl: async () => ({ pairId: 'created' })
}

Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: {
    api: {
      pair: {
        create: async (input: Record<string, unknown>) => {
          api.createCalls.push(input)
          return api.createImpl(input)
        },
        assignTask: async (pairId: string, input: Record<string, unknown>) => {
          api.assignCalls.push({ pairId, input })
          return api.assignImpl(pairId, input)
        },
        updateModels: async (_pairId: string, input: Record<string, unknown>) => input,
        onMessage: async () => {
          return () => undefined
        },
        onState: async (callback: Callback) => {
          api.onState = callback
          return () => undefined
        },
        onHandoff: async () => () => undefined
      },
      session: {
        saveSnapshot: async (input: Record<string, unknown>) => {
          api.saved.push(input)
        },
        loadAllPairs: async () => []
      },
      config: {
        getCachedModels: async () => [],
        refreshModels: async () => []
      },
      insights: {
        recordIntervention: async () => undefined
      }
    }
  }
})

// Silence expected error logs from the failure-path tests.
console.error = () => undefined
console.warn = () => undefined

const now = 1_000

function activity(label: string): AgentActivity {
  return { phase: 'idle', label, startedAt: now, updatedAt: now }
}

function message(id: string, from: Message['from'], timestamp: number): Message {
  return { id, from, to: 'human', type: 'result', content: id, timestamp, iteration: 1 }
}

function makePair(overrides: Partial<Pair> = {}): Pair {
  return {
    id: 'pair-1',
    name: 'Pair One',
    directory: '/tmp/repo/.worktrees/pair-old',
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
    messages: [message('m1', 'mentor', 2_000)],
    mentorActivity: activity('Mentor idle'),
    executorActivity: activity('Executor idle'),
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
    currentRunStartedAt: now,
    currentRunFinishedAt: 5_000,
    branch: 'main',
    repoPath: '/tmp/repo',
    worktreePath: '/tmp/repo/.worktrees/pair-old',
    worktreeBranch: 'the-pair/pair-old',
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

function reset(pairs: Pair[]): void {
  api.saved = []
  api.assignCalls = []
  api.createCalls = []
  api.assignImpl = async () => undefined
  api.createImpl = async () => ({ pairId: 'created' })
  usePairStore.setState({
    pairs,
    availableModels: [grokAlias],
    isLoading: false,
    error: null,
    modelsError: null,
    viewingRunId: null,
    restoringSpec: null
  })
}

function getPair(id = 'pair-1'): Pair {
  const pair = usePairStore.getState().pairs.find((p) => p.id === id)
  assert.ok(pair, `pair ${id} exists`)
  return pair
}

test('createPair forwards the worktree opt-in to the backend', async () => {
  reset([])
  api.createImpl = async () => ({
    pairId: 'pair-wt',
    branch: 'main',
    repoPath: '/tmp/repo',
    worktreePath: '/tmp/repo/.worktrees/pair-wt',
    worktreeBranch: 'the-pair/pair-wt'
  })

  await usePairStore.getState().createPair({
    name: 'Isolated',
    directory: '/tmp/repo',
    spec: 'do the thing',
    mentorModel: 'fast',
    executorModel: 'fast',
    branch: 'main',
    useWorktree: true
  })

  assert.equal(api.createCalls.length, 1)
  assert.equal(api.createCalls[0].useWorktree, true)
  assert.equal(api.createCalls[0].branch, 'main')

  const pair = getPair('pair-wt')
  assert.equal(pair.worktreePath, '/tmp/repo/.worktrees/pair-wt')
  assert.equal(pair.worktreeBranch, 'the-pair/pair-wt')
  assert.equal(pair.repoPath, '/tmp/repo')
  // The pair runs in the worktree, not the user's checkout.
  assert.equal(pair.directory, '/tmp/repo/.worktrees/pair-wt')
})

test('createPair without the opt-in sends no worktree field', async () => {
  reset([])
  api.createImpl = async () => ({ pairId: 'pair-inplace' })

  await usePairStore.getState().createPair({
    name: 'In place',
    directory: '/tmp/repo',
    spec: 'do the thing',
    mentorModel: 'fast',
    executorModel: 'fast'
  })

  assert.equal('useWorktree' in api.createCalls[0], false)
  const pair = getPair('pair-inplace')
  assert.equal(pair.worktreePath, undefined)
  assert.equal(pair.worktreeBranch, undefined)
  assert.equal(pair.directory, '/tmp/repo')
})

test('assignTask forwards freshWorktree and syncs the rotated workspace', async () => {
  reset([makePair()])
  const rotated = {
    pairId: 'pair-1',
    directory: '/tmp/repo/.worktrees/pair-new',
    branch: 'main',
    repoPath: '/tmp/repo',
    worktreePath: '/tmp/repo/.worktrees/pair-new',
    worktreeBranch: 'the-pair/pair-new'
  }
  api.assignImpl = async () => rotated

  await usePairStore
    .getState()
    .assignTask('pair-1', 'next task', undefined, undefined, { freshWorktree: true })

  assert.equal(api.assignCalls.length, 1)
  assert.equal(api.assignCalls[0].input.freshWorktree, true)

  const pair = getPair()
  assert.equal(pair.directory, '/tmp/repo/.worktrees/pair-new')
  assert.equal(pair.worktreePath, '/tmp/repo/.worktrees/pair-new')
  assert.equal(pair.worktreeBranch, 'the-pair/pair-new')
  assert.equal(pair.branch, 'main')

  // The rotated workspace must survive the snapshot the run archiver saves.
  const saved = api.saved.at(-1)
  assert.ok(saved)
  assert.equal(saved.worktreeBranch, 'the-pair/pair-new')
})

test('assignTask without the option sends no worktree field and keeps the workspace', async () => {
  reset([makePair()])
  // The backend response is absent (older shim) — the workspace stays as-is.
  api.assignImpl = async () => undefined

  await usePairStore.getState().assignTask('pair-1', 'next task')

  assert.equal('freshWorktree' in api.assignCalls[0].input, false)

  const pair = getPair()
  assert.equal(pair.directory, '/tmp/repo/.worktrees/pair-old')
  assert.equal(pair.worktreePath, '/tmp/repo/.worktrees/pair-old')
  assert.equal(pair.worktreeBranch, 'the-pair/pair-old')
})
