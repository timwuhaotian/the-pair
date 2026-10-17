/**
 * Component tests for PairConsole transcript search:
 *   - Toggle opens the search bar; typing filters messages + shows count.
 *   - Enter cycles: active message gets the outline; Esc closes + restores.
 *   - Cmd+F opens the search bar.
 */
import './setup-dom.ts'
import './mock-ipc.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach, beforeEach } from 'node:test'
import { I18nextProvider } from 'react-i18next'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import PairConsole from '../../src/renderer/src/components/PairConsole.tsx'
import type { Message, Pair } from '../../src/renderer/src/store/usePairStore.ts'

// jsdom lacks scrollIntoView.
Element.prototype.scrollIntoView = function (): void {}

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

function makePair(messages: Message[]): Pair {
  const activity = {
    phase: 'idle' as const,
    label: 'idle',
    startedAt: 1000,
    updatedAt: 1000,
    outputLineCount: 0
  }
  return {
    id: 'pair-search',
    name: 'Search Pair',
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
    currentRunStartedAt: 1000
  }
}

const MESSAGES = [
  makeMessage({ id: 'm1', content: 'Parser plan ready', from: 'mentor', type: 'plan' }),
  makeMessage({
    id: 'm2',
    content: 'unrelated executor note',
    from: 'executor',
    type: 'progress',
    iteration: 2
  }),
  makeMessage({ id: 'm3', content: 'parser done, Parser verified', from: 'executor' })
]

function renderConsole(): void {
  render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(MESSAGES)} />
    </I18nextProvider>
  )
}

beforeEach(() => {
  void i18n.changeLanguage('en')
})

afterEach(() => {
  cleanup()
})

test('search filters messages, counts occurrences, highlights', () => {
  renderConsole()
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser' } })

  assert.equal(screen.getByTestId('console-search-count').textContent, '3 matches')
  assert.ok(document.getElementById('console-msg-m1'))
  assert.equal(document.getElementById('console-msg-m2'), null)
  assert.ok(document.getElementById('console-msg-m3'))
  assert.equal(document.querySelectorAll('mark').length, 3)
})

test('Enter cycles the active match; Esc closes and restores the transcript', () => {
  renderConsole()
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser' } })

  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-m1')?.className.includes('outline'))
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-m3')?.className.includes('outline'))
  assert.ok(!document.getElementById('console-msg-m1')?.className.includes('outline'))
  fireEvent.keyDown(input, { key: 'Enter', shiftKey: true })
  assert.ok(document.getElementById('console-msg-m1')?.className.includes('outline'))

  fireEvent.keyDown(input, { key: 'Escape' })
  assert.equal(screen.queryByTestId('console-search-input'), null)
  assert.ok(document.getElementById('console-msg-m2'), 'full transcript restored')
  assert.equal(document.querySelectorAll('mark').length, 0)
})

test('search does not count or cycle to the human mission banner', () => {
  const messages = [
    makeMessage({
      id: 'h1',
      content: 'Fix the parser bug',
      from: 'human',
      type: 'feedback',
      iteration: 0
    }),
    makeMessage({ id: 'm1', content: 'parser plan', from: 'mentor', type: 'plan' })
  ]
  render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser' } })

  // Only the mentor card renders a highlighted, anchored match.
  assert.equal(screen.getByTestId('console-search-count').textContent, '1 match')
  assert.equal(document.getElementById('console-msg-h1'), null)
  assert.equal(document.querySelectorAll('mark').length, 1)

  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-m1')?.className.includes('outline'))
})

test('search count ignores messages collapsed away by dedup', () => {
  const messages = [
    makeMessage({ id: 'a', content: 'parser draft', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'b', content: 'parser final', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'c', content: 'parser done', from: 'executor', iteration: 2 })
  ]
  render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser' } })

  assert.equal(document.getElementById('console-msg-a'), null, 'collapsed draft not rendered')
  assert.equal(screen.getByTestId('console-search-count').textContent, '2 matches')
  assert.equal(document.querySelectorAll('mark').length, 2)

  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-b')?.className.includes('outline'))
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-c')?.className.includes('outline'))
})

test('search count, filtered list and highlights agree for link URLs', () => {
  const messages = [
    makeMessage({
      id: 'm1',
      content: 'Edited [parser.ts](src/parser.ts:12) and see [docs](https://example.com/github)'
    })
  ]
  render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')

  // The only "github" hit is the link URL, which renders as an href, not text.
  fireEvent.change(input, { target: { value: 'github' } })
  assert.equal(screen.getByTestId('console-search-count').textContent, 'No matches')
  assert.equal(document.getElementById('console-msg-m1'), null, 'card filtered out')
  assert.equal(document.querySelectorAll('mark').length, 0)

  // Only the link label renders, so exactly one mark and one counted match.
  fireEvent.change(input, { target: { value: 'parser' } })
  assert.equal(screen.getByTestId('console-search-count').textContent, '1 match')
  assert.ok(document.getElementById('console-msg-m1'))
  assert.equal(document.querySelectorAll('mark').length, 1)
})

test('Cmd+F opens the search bar', () => {
  renderConsole()
  fireEvent.keyDown(window, { key: 'f', metaKey: true })
  assert.ok(screen.getByTestId('console-search-input'))
})
