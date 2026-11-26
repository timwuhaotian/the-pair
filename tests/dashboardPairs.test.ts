import assert from 'node:assert/strict'
import test from 'node:test'
import {
  buildPairGroups,
  buildPairInsights,
  buildWorkspaceGroups
} from '../src/renderer/src/lib/dashboardPairs'
import { compactPath } from '../src/renderer/src/lib/workspace'
import type { Pair } from '../src/renderer/src/store/usePairStore'

function pair(status: Pair['status'], overrides: Partial<Pair> = {}): Pair {
  return {
    id: `${status}-${overrides.name ?? 'pair'}`,
    name: overrides.name ?? `${status} pair`,
    directory: overrides.directory ?? '/tmp/project',
    repoPath: overrides.repoPath,
    createdAt: 1,
    status,
    iterations: overrides.iterations ?? 0,
    maxIterations: overrides.maxIterations ?? 6,
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

test('buildWorkspaceGroups shortens Windows paths to the folder name', () => {
  const groups = buildWorkspaceGroups([
    pair('Idle', { name: 'win', directory: 'C:\\Users\\tim\\code\\repo' }),
    pair('Idle', { name: 'win-slash', directory: 'D:\\work\\app\\' }),
    pair('Idle', { name: 'root', directory: '/' })
  ])
  assert.deepEqual(
    groups.map((group) => group.shortName),
    ['repo', 'app', '/']
  )
})

test('buildWorkspaceGroups groups worktree pairs under their repository', () => {
  const groups = buildWorkspaceGroups([
    pair('Finished', {
      name: 'spam',
      directory: '/Users/dev/code/storefront/.worktrees/spam',
      repoPath: '/Users/dev/code/storefront'
    }),
    pair('Executing', {
      name: 'rate',
      directory: '/Users/dev/code/storefront/.worktrees/rate',
      repoPath: '/Users/dev/code/storefront'
    }),
    pair('Idle', { name: 'plain', directory: '/Users/dev/code/billing' })
  ])
  assert.equal(groups.length, 2)
  assert.equal(groups[0].shortName, 'storefront')
  assert.equal(groups[0].pairs.length, 2)
  assert.equal(groups[0].displayPath, '~/code/storefront')
  assert.equal(groups[1].shortName, 'billing')
})

test('compactPath keeps the informative tail of a path', () => {
  assert.equal(compactPath('/Users/dev/code/storefront'), '~/code/storefront')
  assert.equal(compactPath('/home/dev'), '~')
  assert.equal(compactPath('/srv/a/b/c/d'), '…/b/c/d')
  assert.equal(compactPath('/Users/dev/w/x/y/z/'), '~/…/x/y/z')
  assert.equal(compactPath('C:\\Users\\tim\\code\\repo'), '~\\code\\repo')
  assert.equal(compactPath('/'), '/')
  assert.equal(compactPath('/opt/app'), '/opt/app')
})

test('buildPairInsights summarizes useful dashboard context', () => {
  const insights = buildPairInsights([
    pair('Error', {
      modifiedFiles: [{ path: 'a', status: 'M', displayPath: 'a' }]
    }),
    pair('Mentoring', {})
  ])

  assert.equal(insights.needsAttention, 1)
  assert.equal(insights.running, 1)
  assert.equal(insights.modifiedFiles, 1)
  assert.equal(insights.workspaceCount, 1)
})
