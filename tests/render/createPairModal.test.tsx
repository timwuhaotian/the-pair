/**
 * Component unit tests for the CreatePairModal's task / workspace / roles
 * layout, rendered in jsdom:
 *   - the brand header (logo mark + wordmark) and the three section rules
 *   - required-field markers
 *   - the preset hint bar copy (skills + auto-pause)
 *   - actionable error callouts (model list, create failure)
 *   - the preset gate and the payload the modal submits
 *
 * The Tauri IPC layer is mocked (mock-ipc.ts); the store is seeded with stubbed
 * actions so we can assert the exact payload the modal sends.
 */
import './setup-dom.ts'
import './mock-ipc.ts'
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { I18nextProvider } from 'react-i18next'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import {
  CreatePairModal,
  PresetHintBar
} from '../../src/renderer/src/components/CreatePairModal.tsx'
import { usePairStore } from '../../src/renderer/src/store/usePairStore.ts'
import type { PairPreset } from '../../src/renderer/src/types.ts'
import { commitPush, mockRepoState, state } from './mock-ipc.ts'
import { assertAbsent } from './assert-dom.ts'

function setLocale(): void {
  void i18n.changeLanguage('en')
}

const providerStub = {
  id: 'xai-1',
  protocol: 'openaiChat',
  baseUrl: 'https://api.x.ai/v1',
  vendor: 'xai',
  displayName: 'xAI',
  readyAccountCount: 1,
  accounts: [
    {
      id: 'xai-1-a',
      kind: 'apiKey',
      label: 'xai',
      source: 'manual',
      status: 'ready',
      headers: [],
      hasSecret: true
    }
  ],
  models: [
    {
      id: 'fast',
      displayName: 'Fast',
      efforts: [],
      source: 'manual'
    }
  ]
}

/** Seeds the store with a ready model list and inert actions. */
function seedStore(overrides: Record<string, unknown> = {}): void {
  usePairStore.setState({
    providers: [providerStub],
    loadAvailableModels: async () => undefined,
    createPair: async () => undefined,
    error: null,
    modelsError: null,
    ...overrides
  } as never)
}

function wrap(node: React.ReactNode): React.ReactNode {
  return <I18nextProvider i18n={i18n}>{node}</I18nextProvider>
}

function renderModal(): void {
  render(wrap(<CreatePairModal isOpen onClose={() => undefined} />))
}

// jsdom has no per-test DOM reset in node:test; clean up after every test so
// queries never see leftovers from a failed one.
afterEach(() => cleanup())

test('CreatePairModal: renders the task, workspace and roles sections', () => {
  setLocale()
  seedStore()
  renderModal()

  // The sections stay headings, so screen-reader heading navigation still
  // finds them.
  for (const name of ['Task', 'Workspace', 'Roles']) {
    assert.ok(screen.getByRole('heading', { name }))
  }
  // The old one-line explainers are gone.
  assertAbsent(screen.queryByText('What the pair should accomplish'))
})

test('CreatePairModal: each section label is followed by a hairline rule', () => {
  setLocale()
  seedStore()
  renderModal()

  for (const name of ['Task', 'Workspace', 'Roles']) {
    const rule = screen.getByRole('heading', { name }).nextElementSibling
    assert.ok(rule?.className.includes('bg-border'), `${name} has no rule`)
  }
})

test('CreatePairModal: leads with the brand mark and wordmark', () => {
  setLocale()
  seedStore()
  renderModal()

  // The logo mark sits in the dialog's header bar, next to the wordmark.
  const dialog = screen.getByRole('dialog')
  const mark = dialog.querySelector('.brand-mark__art')
  assert.ok(mark, 'the brand mark is missing from the header')
  assert.equal(mark?.getAttribute('viewBox'), '51 116 624 332')
  // The header bar is the dialog's first child.
  assert.ok(dialog.firstElementChild?.contains(mark), 'the mark is not in the header bar')
  assert.ok(screen.getByText('The Pair'))
  assert.ok(screen.getByText('Mentor plans. Executor ships.'))
})

test('CreatePairModal: marks the required fields next to their labels', () => {
  setLocale()
  seedStore()
  renderModal()

  // Name, directory and task specification are the required fields. The
  // control carries the native `required` flag; the label adds the accent
  // asterisk, whose tooltip spells the marker out.
  for (const testId of ['pair-name-input', 'pair-directory-input', 'pair-task-spec']) {
    const field = screen.getByTestId(testId) as HTMLInputElement | HTMLTextAreaElement
    assert.equal(field.required, true, `${testId} is not required`)
    const label = field.labels?.[0]
    assert.ok(label, `${testId} has no label`)
    assert.ok(label.textContent?.includes('*'), `${testId} label lacks the marker`)
    assert.ok(
      label.textContent?.includes('Required') || label.querySelector('[title="Required"]'),
      `${testId} label does not explain the marker`
    )
  }
})

test('CreatePairModal: the toggle rows show their checked state', () => {
  setLocale()
  seedStore()
  renderModal()

  const toggle = screen.getByTestId('plan-gate-toggle')
  assert.match(toggle.textContent ?? '', /\[ \]/)
  fireEvent.click(toggle)
  assert.match(toggle.textContent ?? '', /\[x\]/)
})

test('CreatePairModal: the mention hint explains the @ reference syntax', () => {
  setLocale()
  seedStore()
  renderModal()

  assert.ok(screen.getByText('Type @ to reference workspace files'))
})

test('PresetHintBar: lists the preset skills and its auto-pause point', () => {
  setLocale()
  const preset: PairPreset = {
    id: 'custom',
    name: 'Custom',
    description: 'a preset',
    icon: 'Sparkles',
    mentorPromptTemplate: 'TASK:\n{task}',
    executorPromptTemplate: 'execute',
    recommendedSkills: ['alpha', 'beta'],
    pauseOnIteration: 3
  }

  render(wrap(<PresetHintBar preset={preset} />))

  assert.ok(screen.getByText(/Skills: alpha, beta/))
  assert.ok(screen.getByText(/Auto-pause at iteration 3/))
})

test('CreatePairModal: selecting a preset shows its hint bar', async () => {
  setLocale()
  seedStore()
  renderModal()

  fireEvent.click(await screen.findByTestId('preset-card-bug-fix'))

  assert.ok(screen.getByText(/Using "Bug Fix" preset/))
  assert.ok(screen.getByText(/Auto-pause at iteration 5/))
})

test('CreatePairModal: a model list failure explains itself and retries', async () => {
  setLocale()
  let loads = 0
  seedStore({
    modelsError: 'Failed to load models',
    loadAvailableModels: async () => {
      loads += 1
    }
  })
  renderModal()

  assert.ok(screen.getByText("✗ Couldn't load the model list."))
  assert.ok(
    screen.getByText('Check that your provider CLI is installed and signed in, then retry.')
  )
  // The raw store message stays as the technical detail.
  assert.ok(screen.getByText('Failed to load models'))

  // The modal reloads when it opens, so the retry has to add to that.
  const before = loads
  fireEvent.click(screen.getByRole('button', { name: /Retry/i }))
  await waitFor(() => assert.equal(loads, before + 1))
})

test('CreatePairModal: a create failure shows the reason and the next step', async () => {
  setLocale()
  seedStore()
  renderModal()

  // The modal clears a stale global error when it opens, so the failure has to
  // land after the modal is on screen.
  await act(async () => {
    usePairStore.setState({ error: 'directory is not a git repository' })
  })

  assert.ok(screen.getByText(/directory is not a git repository/))
  assert.ok(screen.getByText('Nothing was created — fix the highlighted fields and try again.'))
})

test('CreatePairModal: an unfilled preset task blocks the submission', async () => {
  setLocale()
  seedStore()
  renderModal()

  fireEvent.click(await screen.findByTestId('preset-card-bug-fix'))

  assert.ok(screen.getByText(/Replace the placeholder in the preset template/))
  const submit = screen.getByTestId('pair-submit-btn') as HTMLButtonElement
  assert.equal(submit.disabled, true)
})

test('CreatePairModal: submitting forwards the workspace and role choices', async () => {
  setLocale()
  state.repo = mockRepoState.committedRepo
  const created: Array<Record<string, unknown>> = []
  seedStore({
    createPair: async (input: Record<string, unknown>) => {
      created.push(input)
    }
  })
  renderModal()

  fireEvent.change(screen.getByTestId('pair-name-input'), { target: { value: 'Isolated' } })
  fireEvent.change(screen.getByTestId('pair-directory-input'), { target: { value: '/repo' } })
  fireEvent.blur(screen.getByTestId('pair-directory-input'))
  fireEvent.change(screen.getByTestId('pair-task-spec'), { target: { value: 'do the thing' } })

  // A committed git repo defaults the worktree opt-in on.
  const toggle = await screen.findByTestId('worktree-toggle')
  await waitFor(() => assert.equal(toggle.getAttribute('aria-pressed'), 'true'))

  fireEvent.click(screen.getByTestId('plan-gate-toggle'))
  fireEvent.click(screen.getByTestId('pair-submit-btn'))

  await waitFor(() => assert.equal(created.length, 1))
  assert.equal(created[0].name, 'Isolated')
  assert.equal(created[0].directory, '/repo')
  assert.equal(created[0].spec, 'do the thing')
  assert.equal(created[0].useWorktree, true)
  assert.equal(created[0].planGate, true)
})

test('CreatePairModal: clicking an empty directory asks the host for a folder', async () => {
  setLocale()
  seedStore()
  renderModal()

  fireEvent.click(screen.getByTestId('pair-directory-input'))

  await waitFor(() =>
    assert.ok(
      commitPush.calls.some((call) => call.cmd.includes('dialog')),
      'the folder picker was never opened'
    )
  )
})

test('CreatePairModal: the footer sits below the scroll region so ROLES rows stay visible', () => {
  setLocale()
  seedStore()
  renderModal()

  // Regression test for the footer-overlap glitch: the submit bar must be an
  // in-flow sibling below the single scroll region, never a sticky overlay
  // that paints over the endpoint rows.
  const submit = screen.getByTestId('pair-submit-btn')
  const footer = submit.parentElement
  assert.ok(footer, 'the submit button has no footer container')
  assert.ok(
    !footer.className.includes('sticky'),
    `footer must not be sticky (got: ${footer.className})`
  )
  const scrollRegion = footer.previousElementSibling
  assert.ok(scrollRegion, 'the footer has no preceding scroll region')
  assert.ok(
    scrollRegion.className.includes('overflow-y-auto'),
    'the content above the footer must be the scroll region'
  )
  assert.ok(
    scrollRegion.className.includes('min-h-0'),
    'the scroll region must shrink inside the bounded form'
  )
  const form = footer.parentElement
  assert.ok(form?.tagName.toLowerCase() === 'form', 'the footer must be a direct form child')
  assert.ok(
    form.className.includes('h-[70vh]'),
    `the form must be height-bounded (got: ${form.className})`
  )
})

test('CreatePairModal: the recommendation hint names the models that worked', async () => {
  setLocale()
  state.recommendation = [
    {
      mentorModel: 'grok:fast',
      executorModel: 'grok:fast',
      runs: 4,
      successes: 3,
      successRate: 0.75
    }
  ]
  try {
    seedStore()
    renderModal()

    fireEvent.change(screen.getByTestId('pair-task-spec'), {
      target: { value: 'refactor the parser module' }
    })

    // The lookup is debounced, so the hint lands a beat later.
    const hint = await screen.findByTestId('recommendation-hint', undefined, { timeout: 3000 })
    assert.match(hint.textContent ?? '', /grok:fast \+ grok:fast succeeded 3\/4 times/)
  } finally {
    state.recommendation = null
  }
})
