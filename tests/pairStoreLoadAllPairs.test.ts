import assert from 'node:assert/strict'
import test from 'node:test'
import { usePairStore } from '../src/renderer/src/store/usePairStore'
import type { PairStatus, SessionSnapshotRecord } from '../src/renderer/src/types'

function snapshot(status: string): SessionSnapshotRecord {
  const now = 1
  const activity = {
    phase: 'idle' as const,
    label: 'Idle',
    startedAt: now,
    updatedAt: now
  }

  return {
    snapshotVersion: 1,
    savedAt: now,
    providerSessions: {},
    pairId: `pair-${status}`,
    name: `Pair ${status}`,
    directory: '/tmp/project',
    spec: '',
    status: status as unknown as PairStatus,
    iterations: 0,
    maxIterations: 6,
    turn: 'mentor',
    mentorProvider: 'codex',
    mentorModel: 'codex',
    executorProvider: 'codex',
    executorModel: 'codex',
    messages: [],
    mentorActivity: activity,
    executorActivity: activity,
    modifiedFiles: [],
    gitTracking: { available: false, baseline: 'base-sha' },
    automationMode: 'full-auto',
    runCount: 0,
    runHistory: [],
    currentRunStartedAt: now,
    createdAt: now
  }
}

test('loadAllPairs normalizes persisted backend statuses before rendering', async () => {
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      api: {
        session: {
          loadAllPairs: async () => [
            snapshot('idle'),
            snapshot('executing'),
            snapshot('reviewing'),
            snapshot('finished')
          ]
        }
      }
    }
  })

  usePairStore.setState({ pairs: [] })

  await usePairStore.getState().loadAllPairs()

  assert.deepEqual(
    usePairStore.getState().pairs.map((pair) => pair.status),
    ['Idle', 'Executing', 'Reviewing', 'Finished']
  )
})

test('loadAllPairs preserves transient live fields and reports quarantined snapshots', async () => {
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      api: {
        session: {
          loadAllPairs: async () => [snapshot('executing')],
          listProblems: async () => [
            { pairId: 'broken', path: '/tmp/broken.json', error: 'truncated' }
          ]
        }
      }
    }
  })
  usePairStore.setState({ pairs: [] })
  await usePairStore.getState().loadAllPairs()
  const existing = usePairStore.getState().pairs[0]
  usePairStore.setState({
    pairs: [
      {
        ...existing,
        currentTurnCard: {
          id: 'turn-live',
          role: 'mentor',
          state: 'live',
          content: 'working',
          activity: existing.mentorActivity,
          startedAt: 1,
          updatedAt: 1,
          cognitiveEvents: []
        },
        adaptiveBudget: 9,
        handoffError: 'previous handoff',
        syncError: 'previous sync'
      }
    ]
  })

  await usePairStore.getState().loadAllPairs()
  await new Promise((resolve) => setImmediate(resolve))
  const pair = usePairStore.getState().pairs[0]
  assert.equal(pair.currentTurnCard?.id, 'turn-live')
  assert.equal(pair.adaptiveBudget, 9)
  assert.equal(pair.gitTracking.baseline, 'base-sha')
  assert.equal(pair.handoffError, 'previous handoff')
  assert.equal(pair.syncError, undefined)
  assert.match(usePairStore.getState().error ?? '', /quarantined 1.*broken/)
})

test('loadAllPairs surfaces backend load failures', async () => {
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      api: {
        session: {
          loadAllPairs: async () => {
            throw new Error('permission denied')
          }
        }
      }
    }
  })
  await usePairStore.getState().loadAllPairs()
  assert.match(usePairStore.getState().error ?? '', /permission denied/)
})
