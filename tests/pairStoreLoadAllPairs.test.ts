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
    mentorCpu: 0,
    mentorMemMb: 0,
    executorCpu: 0,
    executorMemMb: 0,
    cpuUsage: 0,
    memUsage: 0,
    modifiedFiles: [],
    gitTracking: { available: false },
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
