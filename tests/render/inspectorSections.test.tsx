/**
 * Component unit tests for the P1 inspector rework:
 *   - InspectorSection toggles collapse / expand with aria-expanded.
 *   - Modified file rows expand an inline diff (fetched once, cached).
 *   - Diff failures render inline under the row.
 *   - Sections render with the expected test ids and badges.
 */
import './setup-dom.ts'
import './mock-ipc.ts'
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { I18nextProvider } from 'react-i18next'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import PairOperationsPanel from '../../src/renderer/src/components/PairOperationsPanel.tsx'
import { InspectorSection } from '../../src/renderer/src/components/inspector/InspectorSection.tsx'
import type { Pair } from '../../src/renderer/src/store/usePairStore.ts'
import { commitPush } from './mock-ipc.ts'
import { assertAbsent } from './assert-dom.ts'

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

function wrap(node: React.ReactNode): React.ReactNode {
  return <I18nextProvider i18n={i18n}>{node}</I18nextProvider>
}

function renderPanel(pair: Pair): void {
  render(
    wrap(
      <PairOperationsPanel
        pair={pair}
        onPause={async () => undefined}
        onResume={async () => undefined}
        onRestoreTask={() => undefined}
      />
    )
  )
}

afterEach(() => {
  cleanup()
  commitPush.calls.length = 0
  delete commitPush.commit
  delete commitPush.push
})

test('InspectorSection: toggle collapses and re-expands the body', () => {
  setLocale()
  const { unmount } = render(
    wrap(
      <InspectorSection id="demo" label="Demo">
        <span>body content</span>
      </InspectorSection>
    )
  )
  const toggle = screen.getByTestId('inspector-toggle-demo')
  assert.equal(toggle.getAttribute('aria-expanded'), 'true')
  assert.ok(screen.getByText('body content'))

  fireEvent.click(toggle)
  assert.equal(toggle.getAttribute('aria-expanded'), 'false')
  assertAbsent(screen.queryByText('body content'))

  fireEvent.click(toggle)
  assert.equal(toggle.getAttribute('aria-expanded'), 'true')
  assert.ok(screen.getByText('body content'))
  unmount()
  cleanup()
})

test('InspectorSection: defaultOpen=false starts collapsed', () => {
  setLocale()
  const { unmount } = render(
    wrap(
      <InspectorSection id="demo" label="Demo" defaultOpen={false}>
        <span>hidden body</span>
      </InspectorSection>
    )
  )
  assert.equal(screen.getByTestId('inspector-toggle-demo').getAttribute('aria-expanded'), 'false')
  assertAbsent(screen.queryByText('hidden body'))
  unmount()
  cleanup()
})

test('Inspector: files rows expand an inline diff fetched from the backend', async () => {
  setLocale()
  // window.api.repo.getFileDiff routes through the repo_get_file_diff command.
  const diffCalls: string[] = []
  const originalApi = window.api
  window.api = {
    ...originalApi,
    repo: {
      ...originalApi.repo,
      getFileDiff: async (...args: unknown[]) => {
        diffCalls.push(args.join('|'))
        return '+added line'
      }
    }
  }
  try {
    renderPanel(makePair())

    // Files section is open by default with a count badge.
    assert.ok(screen.getByTestId('inspector-section-files'))
    assert.equal(screen.getByTestId('inspector-toggle-files').getAttribute('aria-expanded'), 'true')

    fireEvent.click(screen.getByTestId('ops-file-row-src/a.ts'))
    assert.ok(await screen.findByTestId('ops-file-diff-src/a.ts'))
    assert.ok(await screen.findByText(/added line/))
    assert.deepEqual(diffCalls.length, 1)

    // Collapsing and re-opening serves the cached diff without refetching.
    fireEvent.click(screen.getByTestId('ops-file-row-src/a.ts'))
    assertAbsent(screen.queryByTestId('ops-file-diff-src/a.ts'))
    fireEvent.click(screen.getByTestId('ops-file-row-src/a.ts'))
    assert.ok(await screen.findByTestId('ops-file-diff-src/a.ts'))
    assert.deepEqual(diffCalls.length, 1)
  } finally {
    window.api = originalApi
  }
})

test('Inspector: diff failures render inline under the row', async () => {
  setLocale()
  const originalApi = window.api
  window.api = {
    ...originalApi,
    repo: {
      ...originalApi.repo,
      getFileDiff: async () => {
        throw new Error('diff unavailable')
      }
    }
  }
  try {
    renderPanel(makePair())
    fireEvent.click(screen.getByTestId('ops-file-row-src/a.ts'))
    assert.ok(await screen.findByText(/diff unavailable/))
  } finally {
    window.api = originalApi
  }
})

test('Inspector: run-state and history sections carry status and count badges', () => {
  setLocale()
  const { unmount } = render(
    wrap(
      <PairOperationsPanel
        pair={makePair({
          status: 'Paused',
          runHistory: [
            {
              id: 'run-1',
              spec: 'first run',
              mentorModel: 'grok:fast',
              executorModel: 'grok:fast',
              startedAt: 1_000,
              messages: []
            }
          ]
        })}
        onPause={async () => undefined}
        onResume={async () => undefined}
        onRestoreTask={() => undefined}
      />
    )
  )
  assert.ok(screen.getByTestId('inspector-section-run-state'))
  assert.ok(screen.getByTestId('inspector-section-history'))
  // Paused status badge inside the run-state header.
  assert.match(screen.getByTestId('inspector-section-run-state').textContent ?? '', /Paused/)
  unmount()
  cleanup()
})

test('Inspector: timeline section exposes export actions in its header', async () => {
  setLocale()
  const { unmount } = render(
    wrap(
      <PairOperationsPanel
        pair={makePair({
          messages: [
            {
              id: 'm1',
              from: 'mentor',
              to: 'executor',
              type: 'text',
              content: 'plan',
              timestamp: 1_000,
              iteration: 1
            }
          ]
        })}
        onPause={async () => undefined}
        onResume={async () => undefined}
        onRestoreTask={() => undefined}
      />
    )
  )
  // Timeline starts collapsed; expand it and check the actions render.
  fireEvent.click(screen.getByTestId('inspector-toggle-timeline'))
  await waitFor(() =>
    assert.ok(screen.getByTestId('inspector-section-timeline').querySelector('button[title]'))
  )
  unmount()
  cleanup()
})
