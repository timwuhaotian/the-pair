/**
 * Component tests for PairConsole transcript search:
 *   - Toggle opens the search bar; typing filters messages + shows count.
 *   - Enter cycles: active message gets the outline; Esc closes + restores.
 *   - The app-level Cmd/Ctrl+F event opens the search bar.
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

// jsdom lacks scrollIntoView. Tests assert on the recorded targets.
const scrollTargets: Element[] = []
Element.prototype.scrollIntoView = function (): void {
  scrollTargets.push(this)
}

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

test('Enter scrolls the first highlight of the active card into view', () => {
  scrollTargets.length = 0
  renderConsole()
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser' } })
  assert.equal(scrollTargets.length, 0, 'no scroll before a match is active')

  fireEvent.keyDown(input, { key: 'Enter' })
  assert.equal(scrollTargets.length, 1, 'one scroll per activation')
  const target = scrollTargets[0]
  assert.equal(target.tagName, 'MARK', 'aims at the highlight, not the card')
  assert.ok(target.closest('#console-msg-m1'), 'highlight inside the active card')

  fireEvent.keyDown(input, { key: 'Enter' })
  assert.equal(scrollTargets.length, 2)
  assert.ok(scrollTargets[1].closest('#console-msg-m3'), 'follows the next active card')

  fireEvent.keyDown(input, { key: 'Escape' })
  assert.equal(scrollTargets.length, 2, 'closing search does not scroll')
})

test('re-pressing Enter on a single match scrolls again after scrolling away', () => {
  scrollTargets.length = 0
  const messages = [
    makeMessage({ id: 'm1', content: 'parser plan', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'm2', content: 'unrelated note', from: 'executor', iteration: 2 })
  ]
  render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser' } })

  fireEvent.keyDown(input, { key: 'Enter' })
  assert.equal(scrollTargets.length, 1, 'first activation scrolls')

  // One match means the ordinal (and the active id) never changes; every
  // press must still re-scroll the reader back to it.
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.equal(scrollTargets.length, 2, 'Enter re-scrolls the same match')
  fireEvent.keyDown(input, { key: 'Enter', shiftKey: true })
  assert.equal(scrollTargets.length, 3, 'Shift+Enter re-scrolls the same match')
  fireEvent.click(screen.getByTestId('console-search-next'))
  assert.equal(scrollTargets.length, 4, 'the ▼ button re-scrolls the same match')
  fireEvent.click(screen.getByTestId('console-search-prev'))
  assert.equal(scrollTargets.length, 5, 'the ▲ button re-scrolls the same match')
})

test('search never un-hides a message collapsed away by dedup', () => {
  // Mentor draft `a` is superseded by `b` in the same iteration, so dedup
  // keeps only `b` — and that must hold whether or not the query matches `b`.
  const messages = [
    makeMessage({ id: 'a', content: 'parser draft', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'b', content: 'final plan', from: 'mentor', type: 'plan' })
  ]
  render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  assert.equal(document.getElementById('console-msg-a'), null, 'draft collapsed without search')

  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')

  // Only the draft contains "parser": it stays hidden and the search reports
  // no matches rather than resurrecting the collapsed draft.
  fireEvent.change(input, { target: { value: 'parser' } })
  assert.equal(document.getElementById('console-msg-a'), null, 'draft stays collapsed in search')
  assert.equal(screen.getByTestId('console-search-count').textContent, 'No matches')

  // When the final message matches, the drop marker still reports the draft.
  fireEvent.change(input, { target: { value: 'final' } })
  assert.ok(document.getElementById('console-msg-b'))
  assert.ok(screen.getByText('+ 1 earlier message hidden'))
  assert.equal(screen.getByTestId('console-search-count').textContent, '1 match')
})

test('search finds unbackticked JSX tags that render literally', () => {
  const messages = [
    makeMessage({
      id: 'm1',
      content: 'Wrapped the list in <MessageCard> and returned Array<string> now'
    })
  ]
  render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')

  // CommonMark parses these as inline HTML and react-markdown renders them
  // literally — search must find and highlight them there, not drop the card.
  fireEvent.change(input, { target: { value: 'MessageCard' } })
  assert.equal(screen.getByTestId('console-search-count').textContent, '1 match')
  assert.ok(document.getElementById('console-msg-m1'))
  assert.equal(document.querySelectorAll('mark').length, 1)

  fireEvent.change(input, { target: { value: 'string' } })
  assert.equal(screen.getByTestId('console-search-count').textContent, '1 match')
  assert.ok(document.getElementById('console-msg-m1'))
  assert.equal(document.querySelectorAll('mark').length, 1)
})

test('switching the role filter while searching resets the active match instead of jumping', () => {
  scrollTargets.length = 0
  // One mentor + executor message per iteration: dedup keeps all four, so the
  // match list is [m1, e1, m2, e2] under "all".
  const messages = [
    makeMessage({ id: 'm1', content: 'parser plan one', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'e1', content: 'parser exec one', from: 'executor' }),
    makeMessage({
      id: 'm2',
      content: 'parser plan two',
      from: 'mentor',
      type: 'plan',
      iteration: 2
    }),
    makeMessage({ id: 'e2', content: 'parser exec two', from: 'executor', iteration: 2 })
  ]
  render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser' } })

  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-m1')?.className.includes('outline'))
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-e1')?.className.includes('outline'))
  assert.equal(scrollTargets.length, 2)

  // Narrowing to mentor changes the match list to [m1, m2]: the stale ordinal
  // would outline m2 and scroll to it with no key pressed.
  fireEvent.click(screen.getByTestId('filter-mentor'))
  assert.equal(scrollTargets.length, 2, 'filter switch never scrolls')
  for (const id of ['m1', 'm2', 'e1', 'e2']) {
    assert.ok(
      !document.getElementById(`console-msg-${id}`)?.className.includes('outline'),
      `${id} not outlined after the filter switch`
    )
  }

  // Cycling restarts from the first match of the filtered list.
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-m1')?.className.includes('outline'))
  assert.ok(!document.getElementById('console-msg-m2')?.className.includes('outline'))
  assert.equal(scrollTargets.length, 3)
})

test('the app-level open-search event opens the search bar', () => {
  renderConsole()
  // Cmd/Ctrl+F is matched in App.tsx's shortcut map (exact platform modifier,
  // preventDefault) and dispatched here; the console just listens. The event
  // must come from the jsdom window — Node's global CustomEvent is rejected.
  fireEvent(window, new window.CustomEvent('app:console:open-search'))
  assert.ok(screen.getByTestId('console-search-input'))
})

test('IME Enter and Escape confirm/cancel the composition, not the search', () => {
  renderConsole()
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser' } })

  // WebKit fires the candidate-confirming Enter with keyCode 229 after
  // compositionend, so isComposing is false; only keyCode identifies it.
  fireEvent.keyDown(input, { key: 'Enter', keyCode: 229, isComposing: false })
  assert.ok(
    !document.getElementById('console-msg-m1')?.className.includes('outline'),
    'IME Enter does not activate a match'
  )

  fireEvent.keyDown(input, { key: 'Escape', keyCode: 229, isComposing: false })
  assert.ok(screen.getByTestId('console-search-input'), 'IME Escape does not close search')
  assert.equal(screen.getByTestId('console-search-count').textContent, '3 matches')

  // The same keys outside a composition still act on the search.
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-m1')?.className.includes('outline'))
  fireEvent.keyDown(input, { key: 'Escape' })
  assert.equal(screen.queryByTestId('console-search-input'), null)
})
