import { expect, test } from '@playwright/test'
import { mockCalls, openApp } from '../fixtures/app.ts'

const idleActivity = (label: string) => ({
  phase: 'idle',
  label,
  startedAt: 1_000,
  updatedAt: 1_000,
  outputLineCount: 0
})

const snapshot = {
  pairId: 'pair-commit-e2e',
  name: 'Commit Pair',
  directory: '/repo/.worktrees/pair-commit-e2e',
  spec: 'change things',
  status: 'Finished',
  iterations: 1,
  maxIterations: 0,
  turn: 'mentor',
  mentorModel: 'grok:fast',
  executorModel: 'grok:fast',
  messages: [],
  mentorActivity: idleActivity('Mentor idle'),
  executorActivity: idleActivity('Executor idle'),
  mentorCpu: 0,
  mentorMemMb: 0,
  executorCpu: 0,
  executorMemMb: 0,
  cpuUsage: 0,
  memUsage: 0,
  modifiedFiles: [{ path: 'src/a.ts', status: 'M', displayPath: 'src/a.ts' }],
  gitTracking: { available: true },
  automationMode: 'full-auto',
  runCount: 1,
  runHistory: [],
  currentRunStartedAt: 1_000,
  worktreeBranch: 'the-pair/pair-commit-e2e',
  snapshotVersion: 1,
  savedAt: 1_000,
  providerSessions: {}
}

test.describe('Pair commit & push (headless)', () => {
  test('commits with the typed message and pushes the worktree branch', async ({ page }) => {
    await openApp(page, { snapshots: [snapshot] })

    // Selecting the restored pair opens its detail view.
    await page.getByText('Commit Pair').first().click()
    await expect(page.getByTestId('ops-commit-btn')).toBeVisible()

    await page.getByTestId('ops-commit-input').fill('the-pair: e2e work')
    await page.getByTestId('ops-commit-btn').click()

    const calls = await mockCalls(page)
    const committed = calls.find((c) => c.cmd === 'git_commit_changes')
    expect(committed, 'git_commit_changes was called').toBeTruthy()
    expect(committed?.args).toMatchObject({
      directory: '/repo/.worktrees/pair-commit-e2e',
      message: 'the-pair: e2e work'
    })
    await expect(page.getByTestId('ops-commit-detail')).toContainText('abc1234')

    await expect(page.getByTestId('ops-push-btn')).toContainText('the-pair/pair-commit-e2e')
    await page.getByTestId('ops-push-btn').click()

    const callsAfter = await mockCalls(page)
    const pushed = callsAfter.find((c) => c.cmd === 'git_push_changes')
    expect(pushed, 'git_push_changes was called').toBeTruthy()
    expect(pushed?.args).toMatchObject({ directory: '/repo/.worktrees/pair-commit-e2e' })
    await expect(page.getByTestId('ops-push-detail')).toContainText('the-pair/pair-commit-e2e')
  })
})
