/**
 * Component tests for the PairConsole bottom-pinned composer:
 *   - The task input docks below the transcript scroller, never inside it.
 *   - The plan review bar docks above the input while awaiting review.
 *   - An archived run on screen shows no input.
 *   - Awaiting review on an archived run, the dock offers a way back to the plan.
 *   - The dock resizing re-pins a reader who follows the bottom, and leaves a
 *     reader who has scrolled up where they are.
 */
import './setup-dom.ts'
import './mock-ipc.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach, beforeEach } from 'node:test'
import { I18nextProvider } from 'react-i18next'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import PairConsole from '../../src/renderer/src/components/PairConsole.tsx'
import { usePairStore } from '../../src/renderer/src/store/usePairStore.ts'
import type { Message, Pair, PairRunSummary } from '../../src/renderer/src/store/usePairStore.ts'

function makeMessage(overrides: Partial<Message>): Message {
  return {
    id: 'm1',
    timestamp: 1000,
    from: 'executor',
    to: 'human',
    type: 'result',
    content: 'plain text',
    iteration: 1,
    ...overrides
  }
}

function makePair(messages: Message[], overrides: Partial<Pair> = {}): Pair {
  const activity = {
    phase: 'idle' as const,
    label: 'idle',
    startedAt: 1000,
    updatedAt: 1000,
    outputLineCount: 0
  }
  return {
    id: 'pair-composer',
    name: 'Composer Pair',
    directory: '/repo',
    createdAt: 1000,
    status: 'Finished',
    iterations: 1,
    maxIterations: 0,
    cpuUsage: 0,
    memUsage: 0,
    spec: 'task',
    mentorProvider: 'grok',
    mentorModel: 'grok:fast',
    executorProvider: 'grok',
    executorModel: 'grok:fast',
    messages,
    mentorActivity: activity,
    executorActivity: activity,
    mentorCpu: 0,
    mentorMemMb: 0,
    executorCpu: 0,
    executorMemMb: 0,
    modifiedFiles: [],
    gitTracking: { available: true },
    automationMode: 'full-auto',
    turn: 'mentor',
    runCount: 1,
    runHistory: [],
    currentRunStartedAt: 1000,
    ...overrides
  }
}

const MESSAGES = [
  makeMessage({ id: 'm1', content: 'plan ready', from: 'mentor', type: 'plan' }),
  makeMessage({ id: 'm2', content: 'done', from: 'executor' })
]

function renderConsole(pair: Pair): ReturnType<typeof render> {
  return render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={pair} />
    </I18nextProvider>
  )
}

function assertDocked(el: Element): void {
  const transcript = screen.getByTestId('console-transcript')
  const dock = screen.getByTestId('console-composer-dock')
  assert.equal(transcript.contains(el), false, 'not inside the transcript scroller')
  assert.ok(dock.contains(el), 'inside the bottom dock')
}

beforeEach(() => {
  void i18n.changeLanguage('en')
})

afterEach(() => {
  cleanup()
  usePairStore.getState().setViewingRunId(null)
})

test('the task input docks below the transcript, not inside it', () => {
  renderConsole(makePair(MESSAGES))
  assertDocked(screen.getByTestId('pair-task-input'))
})

test('a fresh session also docks the input at the bottom', () => {
  renderConsole(makePair([]))
  assertDocked(screen.getByTestId('pair-task-input'))
})

test('the plan review bar docks above the input while awaiting review', () => {
  renderConsole(makePair(MESSAGES, { status: 'Awaiting Human Review' }))
  const approve = screen.getByTestId('plan-approve-btn')
  assertDocked(approve)
  const input = screen.getByTestId('pair-task-input')
  assert.ok(
    approve.compareDocumentPosition(input) & Node.DOCUMENT_POSITION_FOLLOWING,
    'review bar sits above the input'
  )
})

const ARCHIVED_RUN: PairRunSummary = {
  id: 'run-arch',
  spec: 'archived task',
  status: 'Finished',
  startedAt: 1000,
  finishedAt: 2000,
  mentorModel: 'grok:fast',
  executorModel: 'grok:fast',
  iterations: 1,
  messages: [makeMessage({ id: 'a1', content: 'archived detail' })]
}

test('an archived run on screen shows no input', () => {
  usePairStore.getState().setViewingRunId('run-arch')
  renderConsole(makePair(MESSAGES, { runHistory: [ARCHIVED_RUN] }))
  assert.ok(document.getElementById('console-msg-a1'), 'archived run rendered')
  assert.equal(screen.queryByTestId('pair-task-input'), null)
})

test('awaiting review on an archived run, the dock returns to the pending plan', () => {
  usePairStore.getState().setViewingRunId('run-arch')
  renderConsole(makePair(MESSAGES, { status: 'Awaiting Human Review', runHistory: [ARCHIVED_RUN] }))
  assert.equal(screen.queryByTestId('plan-approve-btn'), null, 'no approve on an archived run')
  const back = screen.getByText(/Return to review/).closest('button')
  assert.ok(back, 'return-to-review button rendered')
  assertDocked(back)
  act(() => back.click())
  assert.equal(usePairStore.getState().viewingRunId, null)
  assert.ok(screen.getByTestId('plan-approve-btn'), 'back on the live plan review')
})

function withMockResizeObserver(run: (fireResize: () => void) => void): void {
  const callbacks: Array<() => void> = []
  const original = globalThis.ResizeObserver
  globalThis.ResizeObserver = class {
    constructor(cb: () => void) {
      callbacks.push(cb)
    }
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  } as unknown as typeof ResizeObserver
  try {
    run(() => {
      assert.ok(callbacks.length > 0, 'transcript is observed')
      act(() => callbacks.forEach((cb) => cb()))
    })
  } finally {
    globalThis.ResizeObserver = original
  }
}

/** Stubs the transcript's scroll metrics and records every scrollTop write. */
function stubTranscriptScroll(scrollTop: number): number[] {
  const transcript = screen.getByTestId('console-transcript')
  const writes: number[] = []
  Object.defineProperty(transcript, 'scrollHeight', { configurable: true, get: () => 5000 })
  Object.defineProperty(transcript, 'clientHeight', { configurable: true, get: () => 400 })
  Object.defineProperty(transcript, 'scrollTop', {
    configurable: true,
    get: () => scrollTop,
    set: (value: number) => {
      writes.push(value)
    }
  })
  return writes
}

test('the transcript re-pins to the bottom when the dock resizes while following', () => {
  withMockResizeObserver((fireResize) => {
    renderConsole(makePair(MESSAGES))
    const writes = stubTranscriptScroll(0)
    fireResize()
    assert.deepEqual(writes, [5000])
  })
})

test('a dock resize leaves a reader who has scrolled up where they are', () => {
  withMockResizeObserver((fireResize) => {
    renderConsole(makePair(MESSAGES))
    const writes = stubTranscriptScroll(1200)
    // Far from the bottom: the scroll handler clears stick-to-bottom.
    fireEvent.scroll(screen.getByTestId('console-transcript'))
    fireResize()
    assert.deepEqual(writes, [])
  })
})
