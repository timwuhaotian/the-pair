/**
 * Component unit tests for the worktree feature UI, rendered in jsdom:
 *   - CreatePairModal: worktree opt-in, default-on for git repos, preview line
 *   - AssignTaskModal: pair branch display, fresh-worktree toggle + wiring
 *   - BranchPicker: base-branch variant labels
 *   - AppChrome: pair branch chip
 *
 * The Tauri IPC layer is mocked (mock-ipc.ts); the store is seeded with stubbed
 * actions so we can assert the exact payload the components send.
 */
import './setup-dom.ts'
import './mock-ipc.ts'
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { I18nextProvider } from 'react-i18next'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import { CreatePairModal } from '../../src/renderer/src/components/CreatePairModal.tsx'
import { AssignTaskModal } from '../../src/renderer/src/components/AssignTaskModal.tsx'
import { BranchPicker } from '../../src/renderer/src/components/BranchPicker.tsx'
import { AppChrome } from '../../src/renderer/src/components/AppChrome.tsx'
import { usePairStore, type Pair } from '../../src/renderer/src/store/usePairStore.ts'
import { mockRepoState, state } from './mock-ipc.ts'

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
    modifiedFiles: [],
    gitTracking: { available: false },
    automationMode: 'full-auto',
    turn: 'mentor',
    runCount: 1,
    runHistory: [],
    currentRunStartedAt: 1_000,
    ...overrides
  }
}

function wrap(node: React.ReactNode): React.ReactNode {
  return <I18nextProvider i18n={i18n}>{node}</I18nextProvider>
}

// jsdom has no per-test DOM reset in node:test; clean up after every test so
// queries never see leftovers from a failed one.
afterEach(() => cleanup())

test('CreatePairModal: worktree toggle defaults on for a committed git repo', async () => {
  setLocale()
  state.repo = mockRepoState.committedRepo
  const created: Array<Record<string, unknown>> = []
  usePairStore.setState({
    availableModels: [
      {
        provider: 'grok',
        modelId: 'fast',
        displayName: 'Fast',
        providerLabel: 'Grok',
        sourceProviderLabel: 'xAI',
        billingKind: 'byok',
        billingLabel: 'BYOK',
        accessLabel: 'Ready',
        availabilityStatus: 'ready',
        supportsPairExecution: true,
        recommendedRoles: ['mentor', 'executor']
      }
    ],
    loadAvailableModels: async () => undefined,
    createPair: async (input: Record<string, unknown>) => {
      created.push(input)
    },
    error: null,
    modelsError: null
  } as never)

  render(wrap(<CreatePairModal isOpen onClose={() => undefined} />))
  fireEvent.change(screen.getByPlaceholderText('/path/to/project'), {
    target: { value: '/repo' }
  })
  // The directory input commits its git scan on blur.
  fireEvent.blur(screen.getByPlaceholderText('/path/to/project'))

  const toggle = await screen.findByTestId('worktree-toggle')
  await waitFor(() =>
    assert.equal(toggle.getAttribute('aria-pressed'), 'true', 'git repo defaults the toggle on')
  )
  // The base-branch picker and preview line accompany the opt-in.
  assert.ok(await screen.findByText('base branch (defaults to current)'))
  assert.match(
    screen.getByRole('button', { name: /fresh worktree/i }).textContent ?? '',
    /worktree/i
  )
})

test('CreatePairModal: toggling the worktree opt-in off hides the preview', async () => {
  setLocale()
  state.repo = mockRepoState.committedRepo
  usePairStore.setState({
    availableModels: [
      {
        provider: 'grok',
        modelId: 'fast',
        displayName: 'Fast',
        providerLabel: 'Grok',
        sourceProviderLabel: 'xAI',
        billingKind: 'byok',
        billingLabel: 'BYOK',
        accessLabel: 'Ready',
        availabilityStatus: 'ready',
        supportsPairExecution: true,
        recommendedRoles: ['mentor', 'executor']
      }
    ],
    loadAvailableModels: async () => undefined,
    createPair: async () => undefined,
    error: null,
    modelsError: null
  } as never)

  render(wrap(<CreatePairModal isOpen onClose={() => undefined} />))
  fireEvent.change(screen.getByPlaceholderText('/path/to/project'), {
    target: { value: '/repo' }
  })
  fireEvent.blur(screen.getByPlaceholderText('/path/to/project'))

  const toggle = await screen.findByTestId('worktree-toggle')
  await waitFor(() => assert.equal(toggle.getAttribute('aria-pressed'), 'true'))
  // Preview is visible with the toggle on.
  assert.ok(screen.queryByText(/the-pair\/… branch in/))

  fireEvent.click(toggle)
  assert.equal(toggle.getAttribute('aria-pressed'), 'false')
  assert.equal(screen.queryByText(/the-pair\/… branch in/), null)
  // The plain branch picker returns (its repo state loads a tick later).
  assert.ok(await screen.findByText('select branch (optional)'))
})

test('CreatePairModal: submitting forwards the worktree opt-in', async () => {
  setLocale()
  state.repo = mockRepoState.committedRepo
  const created: Array<Record<string, unknown>> = []
  usePairStore.setState({
    availableModels: [
      {
        provider: 'grok',
        modelId: 'fast',
        displayName: 'Fast',
        providerLabel: 'Grok',
        sourceProviderLabel: 'xAI',
        billingKind: 'byok',
        billingLabel: 'BYOK',
        accessLabel: 'Ready',
        availabilityStatus: 'ready',
        supportsPairExecution: true,
        recommendedRoles: ['mentor', 'executor']
      }
    ],
    loadAvailableModels: async () => undefined,
    createPair: async (input: Record<string, unknown>) => {
      created.push(input)
    },
    error: null,
    modelsError: null
  } as never)

  const { unmount } = render(wrap(<CreatePairModal isOpen onClose={() => undefined} />))
  fireEvent.change(screen.getByPlaceholderText('/path/to/project'), {
    target: { value: '/repo' }
  })
  fireEvent.blur(screen.getByPlaceholderText('/path/to/project'))
  fireEvent.change(screen.getByTestId('pair-name-input'), { target: { value: 'Isolated' } })
  fireEvent.change(screen.getByTestId('pair-task-spec'), {
    target: { value: 'do the isolated thing' }
  })

  const toggle = await screen.findByTestId('worktree-toggle')
  await waitFor(() => assert.equal(toggle.getAttribute('aria-pressed'), 'true'))
  fireEvent.click(screen.getByTestId('pair-submit-btn'))

  await waitFor(() => assert.equal(created.length, 1))
  assert.equal(created[0].useWorktree, true)
  unmount()
})

test('AssignTaskModal: resets the fresh-worktree opt-in across sessions', async () => {
  setLocale()
  state.repo = mockRepoState.committedRepo
  usePairStore.setState({ restoringSpec: null } as never)
  const pair = makePair({
    branch: 'main',
    repoPath: '/repo',
    worktreeBranch: 'the-pair/pair-ui'
  })

  // Starts closed: the repo check effect takes the early return.
  const view = render(
    wrap(<AssignTaskModal pair={pair} isOpen={false} onClose={() => undefined} />)
  )
  view.rerender(wrap(<AssignTaskModal pair={pair} isOpen onClose={() => undefined} />))

  const toggle = await screen.findByTestId('fresh-worktree-toggle')
  fireEvent.click(toggle)
  assert.equal(toggle.getAttribute('aria-pressed'), 'true')

  // Closing and reopening starts a fresh session with the opt-in cleared.
  view.rerender(wrap(<AssignTaskModal pair={pair} isOpen={false} onClose={() => undefined} />))
  view.rerender(wrap(<AssignTaskModal pair={pair} isOpen onClose={() => undefined} />))
  const reopened = await screen.findByTestId('fresh-worktree-toggle')
  await waitFor(() => assert.equal(reopened.getAttribute('aria-pressed'), 'false'))
})

test('AssignTaskModal: shows the pair branch and offers the fresh worktree toggle', async () => {
  setLocale()
  state.repo = mockRepoState.committedRepo
  const calls: Array<{ pairId: string; spec: string; role?: string; options?: unknown }> = []
  usePairStore.setState({
    availableModels: [],
    restoringSpec: null,
    assignTask: async (
      pairId: string,
      spec: string,
      role?: string,
      _overrides?: unknown,
      options?: unknown
    ) => {
      calls.push({ pairId, spec, role, options })
    }
  } as never)

  const pair = makePair({
    branch: 'main',
    repoPath: '/repo',
    worktreeBranch: 'the-pair/pair-ui'
  })
  const { unmount } = render(wrap(<AssignTaskModal pair={pair} isOpen onClose={() => undefined} />))

  // The workspace box surfaces the pair's working branch and its base.
  await waitFor(() => assert.ok(screen.getByText('the-pair/pair-ui')))
  assert.match(screen.getByText('from main').textContent ?? '', /from main/)

  const toggle = await screen.findByTestId('fresh-worktree-toggle')
  fireEvent.click(toggle)
  assert.equal(toggle.getAttribute('aria-pressed'), 'true')

  fireEvent.change(screen.getByTestId('assign-task-spec'), {
    target: { value: 'the next isolated task' }
  })
  fireEvent.click(screen.getByTestId('assign-submit-btn'))

  await waitFor(() => assert.equal(calls.length, 1))
  assert.equal(calls[0].spec, 'the next isolated task')
  assert.equal(calls[0].role, undefined)
  assert.deepEqual(calls[0].options, { freshWorktree: true })
  unmount()
})

test('AssignTaskModal: hides the fresh worktree toggle for a non-git workspace', async () => {
  setLocale()
  state.repo = mockRepoState.emptyRepo
  usePairStore.setState({ restoringSpec: null } as never)

  const pair = makePair({ branch: undefined, repoPath: undefined, worktreeBranch: undefined })
  const { unmount } = render(wrap(<AssignTaskModal pair={pair} isOpen onClose={() => undefined} />))

  await waitFor(() => assert.ok(screen.getByTestId('assign-task-spec')))
  assert.equal(screen.queryByTestId('fresh-worktree-toggle'), null)
  unmount()
})

test('AssignTaskModal: hides the fresh worktree toggle when the repo state is unreadable', async () => {
  setLocale()
  state.failRepoCheck = true
  usePairStore.setState({ restoringSpec: null } as never)

  const pair = makePair({ branch: undefined, repoPath: undefined, worktreeBranch: undefined })
  const { unmount } = render(wrap(<AssignTaskModal pair={pair} isOpen onClose={() => undefined} />))

  await waitFor(() => assert.ok(screen.getByTestId('assign-task-spec')))
  assert.equal(screen.queryByTestId('fresh-worktree-toggle'), null)
  unmount()
  cleanup()
  state.failRepoCheck = false
})

test('BranchPicker: the base variant labels the empty selection as current', async () => {
  setLocale()
  state.repo = mockRepoState.committedRepo
  const { unmount } = render(
    wrap(<BranchPicker directory="/repo" onChange={() => undefined} variant="base" />)
  )

  await waitFor(() => assert.ok(screen.getByText('base branch (defaults to current)')))
  fireEvent.click(screen.getByText('base branch (defaults to current)'))
  await waitFor(() => assert.ok(screen.getByText('— from current branch')))
  // The current branch is still selectable even though the repo is clean.
  assert.ok(screen.getByText('main'))
  unmount()
})

test('BranchPicker: the base variant explains the dirty-repo restriction', async () => {
  setLocale()
  state.repo = { ...mockRepoState.committedRepo, isDirty: true }
  const { unmount } = render(
    wrap(<BranchPicker directory="/repo" onChange={() => undefined} variant="base" />)
  )

  await screen.findByText('base branch (defaults to current)')
  fireEvent.click(screen.getByText('base branch (defaults to current)'))
  await waitFor(() =>
    assert.match(
      screen.getByText(/only the current branch can be the base/).textContent ?? '',
      /uncommitted changes/
    )
  )
  unmount()
  cleanup()

  // The legacy (non-base) variant keeps the original wording.
  const view = render(wrap(<BranchPicker directory="/repo" onChange={() => undefined} />))
  await screen.findByText('select branch (optional)')
  fireEvent.click(screen.getByText('select branch (optional)'))
  await waitFor(() =>
    assert.match(
      screen.getByText(/commit or stash before selecting a branch/).textContent ?? '',
      /uncommitted changes/
    )
  )
  view.unmount()
  cleanup()
})

test('AppChrome: shows a branch chip for a pair with a worktree branch', () => {
  setLocale()
  const pair = makePair({ worktreeBranch: 'the-pair/pair-ui' })
  const { unmount } = render(
    wrap(
      <AppChrome
        selectedPair={pair}
        readyModelCount={1}
        totalModelCount={1}
        theme="dark"
        onToggleTheme={() => undefined}
        onNewTask={() => undefined}
        onBack={() => undefined}
        onOpenSettings={() => undefined}
      />
    )
  )
  const chip = screen.getByTestId('chrome-branch-chip')
  assert.match(chip.textContent ?? '', /the-pair\/pair-ui/)
  unmount()
})

test('AppChrome: hides the branch chip for an in-place pair', () => {
  setLocale()
  const { unmount } = render(
    wrap(
      <AppChrome
        selectedPair={makePair()}
        readyModelCount={1}
        totalModelCount={1}
        theme="dark"
        onToggleTheme={() => undefined}
        onNewTask={() => undefined}
        onBack={() => undefined}
        onOpenSettings={() => undefined}
      />
    )
  )
  assert.equal(screen.queryByTestId('chrome-branch-chip'), null)
  unmount()
})
