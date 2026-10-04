/**
 * Component unit tests for the pair commit & push UI in PairOperationsPanel:
 *   - Commit button sends directory + message and shows the returned SHA.
 *   - Commit failures render inline.
 *   - Push button shows the worktree branch and reports pushed / up-to-date.
 *   - Push hides when the pair has no known branch.
 *
 * The Tauri IPC layer is mocked (mock-ipc.ts, commitPush script); the panel
 * talks to window.api directly so no store seeding is needed.
 */
import './setup-dom.ts'
import './mock-ipc.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { I18nextProvider } from 'react-i18next'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import PairOperationsPanel from '../../src/renderer/src/components/PairOperationsPanel.tsx'
import type { Pair } from '../../src/renderer/src/store/usePairStore.ts'
import { commitPush } from './mock-ipc.ts'

function setLocale(): void {
  void i18n.changeLanguage('en')
}

function makePair(overrides: Partial<Pair> = {}): Pair {
  return {
    id: 'pair-ui',
    name: 'UI Pair',
    directory: '/repo/.worktrees/pair-ui',
    createdAt: 1_000,
    status: 'Finished',
    iterations: 3,
    maxIterations: 0,
    cpuUsage: 0,
    memUsage: 0,
    spec: 'some task',
    mentorProvider: 'grok',
    mentorModel: 'grok:fast',
    executorProvider: 'grok',
    executorModel: 'grok:fast',
    messages: [],
    mentorActivity: {
      phase: 'idle',
      label: 'Mentor idle',
      startedAt: 1_000,
      updatedAt: 1_000,
      outputLineCount: 0
    },
    executorActivity: {
      phase: 'idle',
      label: 'Executor idle',
      startedAt: 1_000,
      updatedAt: 1_000,
      outputLineCount: 0
    },
    mentorCpu: 0,
    mentorMemMb: 0,
    executorCpu: 0,
    executorMemMb: 0,
    modifiedFiles: [{ path: 'src/a.ts', displayPath: 'src/a.ts', status: 'M' }],
    gitTracking: { available: true },
    automationMode: 'full-auto',
    turn: 'mentor',
    runCount: 1,
    runHistory: [],
    currentRunStartedAt: 1_000,
    worktreeBranch: 'the-pair/pair-ui',
    ...overrides
  }
}

function renderPanel(pair: Pair): void {
  render(
    <I18nextProvider i18n={i18n}>
      <PairOperationsPanel
        pair={pair}
        onPause={async () => undefined}
        onResume={async () => undefined}
        onRestoreTask={() => undefined}
      />
    </I18nextProvider>
  )
}

// jsdom has no per-test DOM reset in node:test; clean up after every test so
// queries never see leftovers from a failed one.
afterEach(() => {
  cleanup()
  commitPush.calls.length = 0
  delete commitPush.commit
  delete commitPush.push
})

test('Commit: sends directory + message and shows the returned SHA', async () => {
  setLocale()
  renderPanel(makePair())

  const input = screen.getByTestId('ops-commit-input')
  const button = screen.getByTestId('ops-commit-btn')
  // Blank message keeps the button disabled.
  assert.equal(button.hasAttribute('disabled'), true)
  fireEvent.change(input, { target: { value: 'the-pair: ship it' } })
  assert.equal(button.hasAttribute('disabled'), false)
  fireEvent.click(button)

  await waitFor(() =>
    assert.ok(
      commitPush.calls.some(
        (c) =>
          c.cmd === 'git_commit_changes' &&
          (c.args as { directory: string }).directory === '/repo/.worktrees/pair-ui' &&
          (c.args as { message: string }).message === 'the-pair: ship it'
      ),
      'commit invoked with directory + message'
    )
  )
  assert.ok(await screen.findByText(/Committed abc1234/))
})

test('Commit: failures render inline', async () => {
  setLocale()
  commitPush.commit = { error: 'git commit failed: author identity unknown' }
  renderPanel(makePair())

  fireEvent.change(screen.getByTestId('ops-commit-input'), { target: { value: 'work' } })
  fireEvent.click(screen.getByTestId('ops-commit-btn'))

  assert.ok(await screen.findByText(/author identity unknown/))
})

test('Push: button shows the worktree branch and reports the push', async () => {
  setLocale()
  renderPanel(makePair())

  const button = screen.getByTestId('ops-push-btn')
  assert.match(button.textContent ?? '', /the-pair\/pair-ui/)
  fireEvent.click(button)

  await waitFor(() =>
    assert.ok(
      commitPush.calls.some(
        (c) =>
          c.cmd === 'git_push_changes' &&
          (c.args as { directory: string }).directory === '/repo/.worktrees/pair-ui'
      ),
      'push invoked with directory'
    )
  )
  assert.ok(await screen.findByText(/Pushed the-pair\/pair-ui/))
})

test('Push: up-to-date reports without error', async () => {
  setLocale()
  commitPush.push = { branch: 'the-pair/pair-ui', remote: 'origin', upToDate: true }
  renderPanel(makePair())

  fireEvent.click(screen.getByTestId('ops-push-btn'))
  assert.ok(await screen.findByText(/already up to date/))
})

test('Push: hidden when the pair has no known branch', async () => {
  setLocale()
  renderPanel(makePair({ worktreeBranch: undefined, branch: undefined }))

  assert.equal(screen.queryByTestId('ops-push-btn'), null)
  // Commit still works without a branch.
  assert.ok(screen.getByTestId('ops-commit-btn'))
})
