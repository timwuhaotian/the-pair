import assert from 'node:assert/strict'
import test from 'node:test'
import {
  buildPairGroups,
  buildPairInsights,
  buildWorkspaceGroups
} from '../src/renderer/src/lib/dashboardPairs'
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

test('buildWorkspaceGroups buckets pairs by directory and subgroups by status', () => {
  const pairs = [
    pair('Mentoring', { name: 'a-running', directory: '/repo/alpha' }),
    pair('Idle', { name: 'a-ready', directory: '/repo/alpha' }),
    pair('Error', { name: 'b-err', directory: '/repo/beta' }),
    pair('Finished', { name: 'b-done', directory: '/repo/beta/' })
  ]

  const groups = buildWorkspaceGroups(pairs)

  assert.equal(groups.length, 3, 'each unique directory string forms its own workspace bucket')
  const alpha = groups.find((g) => g.directory === '/repo/alpha')
  assert.ok(alpha)
  assert.equal(alpha.shortName, 'alpha')
  assert.equal(alpha.pairs.length, 2)
  assert.deepEqual(
    alpha.statusGroups.map((g) => g.key),
    ['active', 'ready']
  )

  const betaWithSlash = groups.find((g) => g.directory === '/repo/beta/')
  assert.ok(betaWithSlash)
  assert.equal(betaWithSlash.shortName, 'beta')
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
