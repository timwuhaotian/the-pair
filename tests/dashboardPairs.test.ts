import assert from 'node:assert/strict'
import test from 'node:test'
import { buildPairGroups, buildPairInsights } from '../src/renderer/src/lib/dashboardPairs'
import type { Pair } from '../src/renderer/src/store/usePairStore'

function pair(status: Pair['status'], overrides: Partial<Pair> = {}): Pair {
  return {
    id: `${status}-${overrides.name ?? 'pair'}`,
    name: overrides.name ?? `${status} pair`,
    directory: overrides.directory ?? '/tmp/project',
    createdAt: 1,
    status,
    iterations: overrides.iterations ?? 0,
    maxIterations: overrides.maxIterations ?? 6,
    cpuUsage: overrides.cpuUsage ?? 0,
    memUsage: overrides.memUsage ?? 0,
    spec: '',
    mentorProvider: 'codex',
    mentorModel: 'codex',
    executorProvider: 'codex',
    executorModel: 'codex',
    messages: [],
    mentorActivity: {
      phase: 'idle',
      label: '',
      startedAt: 1,
      updatedAt: 1
    },
    executorActivity: {
      phase: 'idle',
      label: '',
      startedAt: 1,
      updatedAt: 1
    },
    mentorCpu: 0,
    mentorMemMb: 0,
    executorCpu: 0,
    executorMemMb: 0,
    modifiedFiles: overrides.modifiedFiles ?? [],
    gitTracking: { available: false },
    automationMode: 'full-auto',
    turn: 'mentor',
    runCount: 0,
    runHistory: [],
    currentRunStartedAt: 1
  }
}

test('buildPairGroups keeps every status visible', () => {
  const pairs = [
    pair('Error'),
    pair('Awaiting Human Review'),
    pair('Idle'),
    pair('Finished'),
    pair('Mentoring'),
    pair('Paused')
  ]

  const groupedPairs = buildPairGroups(pairs).flatMap((group) => group.pairs)

  assert.deepEqual(groupedPairs.map((p) => p.status).sort(), pairs.map((p) => p.status).sort())
})

test('buildPairInsights summarizes useful dashboard context', () => {
  const insights = buildPairInsights([
    pair('Error', {
      cpuUsage: 11.4,
      memUsage: 120,
      modifiedFiles: [{ path: 'a', status: 'M', displayPath: 'a' }]
    }),
    pair('Mentoring', { cpuUsage: 3.1, memUsage: 80 })
  ])

  assert.equal(insights.needsAttention, 1)
  assert.equal(insights.running, 1)
  assert.equal(insights.modifiedFiles, 1)
  assert.equal(insights.cpuUsage, 14.5)
  assert.equal(insights.memUsage, 200)
})
