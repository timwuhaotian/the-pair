/**
 * Component tests for the hub pair card. The name button's ::after covers the
 * whole card, so it is the hover target for every non-action row: the card's
 * tooltip has to live on that button for the truncated spec to stay readable.
 */
import './setup-dom.ts'
import './mock-ipc.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { I18nextProvider } from 'react-i18next'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import { PairsHub } from '../../src/renderer/src/components/PairsHub.tsx'
import type { Pair } from '../../src/renderer/src/store/usePairStore.ts'

function makePair(overrides: Partial<Pair> = {}): Pair {
  return {
    id: 'pair-hub',
    name: 'Hub Pair',
    directory: '/repo/hub',
    createdAt: 1_000,
    status: 'Finished',
    iterations: 3,
    maxIterations: 0,
    cpuUsage: 0,
    memUsage: 0,
    spec: 'Rewrite the settings screen so every section shares one layout',
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

function renderHub(pairs: Pair[], onSelectPair: (pair: Pair) => void = () => undefined): void {
  void i18n.changeLanguage('en')
  render(
    <I18nextProvider i18n={i18n}>
      <PairsHub
        pairs={pairs}
        onSelectPair={onSelectPair}
        onDeletePair={() => undefined}
        onPausePair={() => undefined}
        onResumePair={() => undefined}
        onCreatePair={() => undefined}
        deletingPairId={null}
      />
    </I18nextProvider>
  )
}

afterEach(cleanup)

test('the card target carries the full spec as its tooltip', () => {
  const pair = makePair()
  renderHub([pair])
  assert.equal(screen.getByRole('button', { name: 'Hub Pair' }).title, pair.spec)
})

test('a pair without a spec shows its directory as the tooltip', () => {
  renderHub([makePair({ id: 'no-spec', name: 'Bare Pair', spec: '' })])
  assert.equal(screen.getByRole('button', { name: 'Bare Pair' }).title, '/repo/hub')
})

test('clicking the card target selects the pair', () => {
  const selected: string[] = []
  renderHub([makePair()], (pair) => selected.push(pair.id))
  fireEvent.click(screen.getByRole('button', { name: 'Hub Pair' }))
  assert.deepEqual(selected, ['pair-hub'])
})
