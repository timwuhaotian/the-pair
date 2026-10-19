/**
 * Component tests for PairConsole transcript search:
 *   - Toggle opens the search bar; typing filters messages + shows count.
 *   - Enter cycles: active message gets the outline; Esc closes + restores.
 *   - The app-level Cmd/Ctrl+F event opens the search bar.
 *   - Enter pressed while the deferred list lags the typed query waits for
 *     the list instead of cycling the stale one (concurrent root, no act).
 */
import './setup-dom.ts'
import './mock-ipc.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach, beforeEach } from 'node:test'
import { I18nextProvider } from 'react-i18next'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { createRoot, type Root } from 'react-dom/client'
import i18n from '../../src/renderer/src/i18n.ts'
import PairConsole from '../../src/renderer/src/components/PairConsole.tsx'
import { usePairStore } from '../../src/renderer/src/store/usePairStore.ts'
import type { Message, Pair, PairRunSummary } from '../../src/renderer/src/store/usePairStore.ts'

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

function makePairWithRuns(messages: Message[], archivedRun: PairRunSummary): Pair {
  return { ...makePair(messages), runHistory: [archivedRun] }
}

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

test('search count, filtered list and highlights agree for excess table cells', () => {
  // GFM tables render only header-count cells per body row: the dropped cell
  // text must not keep the card listed with nothing highlighted.
  const messages = [
    makeMessage({
      id: 'm1',
      content: '| file | status |\n| --- | --- |\n| `parser.ts` | ok | failed | extra |'
    })
  ]
  render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')

  fireEvent.change(input, { target: { value: 'extra' } })
  assert.equal(screen.getByTestId('console-search-count').textContent, 'No matches')
  assert.equal(document.getElementById('console-msg-m1'), null, 'card filtered out')
  assert.equal(document.querySelectorAll('mark').length, 0)

  fireEvent.change(input, { target: { value: 'ok' } })
  assert.equal(screen.getByTestId('console-search-count').textContent, '1 match')
  assert.ok(document.getElementById('console-msg-m1'))
  assert.equal(document.querySelectorAll('mark').length, 1)
})

test('search count, filtered list and highlights agree for footnote definitions', () => {
  // Only referenced definitions render (in the hast footer); an unreferenced
  // one's text must not be counted, a referenced one's must.
  const messages = [
    makeMessage({
      id: 'm1',
      content: 'Note: the parser needs an options bag.\n\n[^1]: parser detail here'
    }),
    makeMessage({
      id: 'm2',
      content: 'See the note[^1] about options.\n\n[^1]: parser detail here',
      iteration: 2
    })
  ]
  render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')

  // The unreferenced definition under m1 stays uncounted: 1 body hit only.
  fireEvent.change(input, { target: { value: 'parser' } })
  assert.equal(screen.getByTestId('console-search-count').textContent, '2 matches')
  assert.ok(document.getElementById('console-msg-m1'))
  assert.ok(document.getElementById('console-msg-m2'))
  assert.equal(document.querySelectorAll('mark').length, 2)

  // A query hitting only the unreferenced definition's text finds nothing.
  fireEvent.change(input, { target: { value: 'detail here' } })
  assert.equal(screen.getByTestId('console-search-count').textContent, '1 match')
  assert.equal(document.getElementById('console-msg-m1'), null, 'unreferenced footnote not listed')
  assert.ok(document.getElementById('console-msg-m2'), 'referenced footnote listed')
  const marks = document.querySelectorAll('mark')
  assert.equal(marks.length, 1)
  assert.ok(marks[0].closest('#console-msg-m2'), 'footer hit highlighted on the referencing card')
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

test('editing the query after cycling resets the active match instead of jumping', () => {
  scrollTargets.length = 0
  // Same shape as the filter-switch test: dedup keeps all four, so the match
  // list is [m1, e1, m2, e2] under "parser" but only [m2, e2] under "parser.".
  const messages = [
    makeMessage({ id: 'm1', content: 'parser plan one', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'e1', content: 'parser exec one', from: 'executor' }),
    makeMessage({
      id: 'm2',
      content: 'parser.ts plan two',
      from: 'mentor',
      type: 'plan',
      iteration: 2
    }),
    makeMessage({ id: 'e2', content: 'parser.ts exec two', from: 'executor', iteration: 2 })
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

  // Refining the query: the stale ordinal would point at e2 in the new match
  // list and outline + scroll to it with no key pressed.
  fireEvent.change(input, { target: { value: 'parser.' } })
  assert.equal(scrollTargets.length, 2, 'query edit never scrolls')
  for (const id of ['m1', 'm2', 'e1', 'e2']) {
    assert.ok(
      !document.getElementById(`console-msg-${id}`)?.className.includes('outline'),
      `${id} not outlined after the query edit`
    )
  }
  assert.equal(screen.getByTestId('console-search-count').textContent, '2 matches')

  // Cycling restarts from the first match of the refined list.
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-m2')?.className.includes('outline'))
  assert.equal(scrollTargets.length, 3)
})

test('a live append that supersedes the active match deactivates it instead of re-indexing', () => {
  scrollTargets.length = 0
  // Match list [p1, p2, p3] under "parser"; the run is live, and a revised
  // executor result for the same iteration is about to supersede p2.
  const messages = [
    makeMessage({ id: 'p1', content: 'parser plan', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'p2', content: 'parser fixed', from: 'executor' }),
    makeMessage({ id: 'p3', content: 'parser verified', from: 'executor', iteration: 2 })
  ]
  const view = render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser' } })

  fireEvent.keyDown(input, { key: 'Enter' })
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-p2')?.className.includes('outline'))
  assert.equal(scrollTargets.length, 2)

  // p2b supersedes p2 in dedup while the (pair, run, query) scope is
  // unchanged: a kept index would resolve to p3 in [p1, p3, p2b] and
  // outline + scroll to it with no cycle press. The store always hands the
  // console a fresh messages array, so the rerender does too.
  const afterAppend = [
    ...messages,
    makeMessage({ id: 'p2b', content: 'parser fixed, revised', from: 'executor' })
  ]
  view.rerender(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(afterAppend)} />
    </I18nextProvider>
  )

  assert.equal(document.getElementById('console-msg-p2'), null, 'superseded card collapsed away')
  assert.equal(scrollTargets.length, 2, 'the append never scrolls')
  for (const id of ['p1', 'p2b', 'p3']) {
    assert.ok(
      !document.getElementById(`console-msg-${id}`)?.className.includes('outline'),
      `${id} not outlined after the append`
    )
  }

  // Cycling restarts from the first match of the revised list.
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-p1')?.className.includes('outline'))
  assert.equal(scrollTargets.length, 3)
})

test('a live append that keeps the active card anchors the activation by id', () => {
  scrollTargets.length = 0
  const messages = [
    makeMessage({ id: 'p1', content: 'parser plan', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'p2', content: 'parser fixed', from: 'executor' }),
    makeMessage({ id: 'p3', content: 'parser verified', from: 'executor', iteration: 2 })
  ]
  const view = render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser' } })

  fireEvent.keyDown(input, { key: 'Enter' })
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-p2')?.className.includes('outline'))
  assert.equal(scrollTargets.length, 2)

  // A new iteration's result appends without touching p2: the match list
  // grows to [p1, p2, p3, p4], and the activation must stay on the card the
  // reader cycled to rather than whatever now sits at the old index.
  const afterAppend = [
    ...messages,
    makeMessage({ id: 'p4', content: 'parser shipped', from: 'executor', iteration: 3 })
  ]
  view.rerender(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(afterAppend)} />
    </I18nextProvider>
  )

  assert.equal(scrollTargets.length, 2, 'the append never scrolls')
  assert.ok(
    document.getElementById('console-msg-p2')?.className.includes('outline'),
    'activation stays anchored to the same card'
  )

  // Enter advances from the anchor within the grown list.
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-p3')?.className.includes('outline'))
  assert.ok(!document.getElementById('console-msg-p2')?.className.includes('outline'))
  assert.equal(scrollTargets.length, 3)
})

test('live output does not yank the reader off an active search match', () => {
  const messages = [
    makeMessage({ id: 'm1', content: 'parser plan', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'm2', content: 'unrelated note', from: 'executor', iteration: 2 })
  ]
  const view = render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser' } })
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.ok(document.getElementById('console-msg-m1')?.className.includes('outline'))

  // jsdom's scroller reports 0/0; record writes against a tall stub instead.
  const scroller = view.container.querySelector<HTMLElement>('div.overflow-y-auto')
  assert.ok(scroller)
  const writes: number[] = []
  Object.defineProperty(scroller, 'scrollHeight', { configurable: true, get: () => 5000 })
  Object.defineProperty(scroller, 'scrollTop', {
    configurable: true,
    get: () => (writes.length > 0 ? writes[writes.length - 1] : 0),
    set: (value: number) => {
      writes.push(value)
    }
  })

  // A streamed append while the match is active must not snap to the bottom.
  view.rerender(
    <I18nextProvider i18n={i18n}>
      <PairConsole
        pair={makePair([
          ...messages,
          makeMessage({ id: 'm3', content: 'parser shipped', from: 'executor', iteration: 3 })
        ])}
      />
    </I18nextProvider>
  )
  assert.ok(
    document.getElementById('console-msg-m1')?.className.includes('outline'),
    'the active match stays active across the append'
  )
  assert.deepEqual(writes, [], 'no auto-follow write while a search is active')

  // Closing the search returns the console to live following.
  fireEvent.click(screen.getByTestId('console-search-close'))
  assert.deepEqual(writes, [5000], 'follow resumes at the bottom once search closes')
})

test('cycling with no matches never activates or scrolls', () => {
  scrollTargets.length = 0
  const messages = [makeMessage({ id: 'm1', content: 'unrelated note' })]
  render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser' } })
  assert.equal(screen.getByTestId('console-search-count').textContent, 'No matches')

  fireEvent.keyDown(input, { key: 'Enter' })
  fireEvent.keyDown(input, { key: 'Enter', shiftKey: true })
  assert.ok(!document.getElementById('console-msg-m1')?.className.includes('outline'))
  assert.equal(scrollTargets.length, 0)
})

test('the app-level open-search event opens the search bar', () => {
  renderConsole()
  // Cmd/Ctrl+F is matched in App.tsx's shortcut map (exact platform modifier,
  // preventDefault) and dispatched here; the console just listens. The event
  // must come from the jsdom window — Node's global CustomEvent is rejected.
  fireEvent(window, new window.CustomEvent('app:console:open-search'))
  assert.ok(screen.getByTestId('console-search-input'))
})

test('the open-search event never steals focus from the clear-history confirm', () => {
  renderConsole()
  fireEvent.click(screen.getByTestId('console-clear-btn'))
  const dialog = screen.getByTestId('confirm-modal-cancel').closest('[role="dialog"]')
  assert.ok(dialog, 'the clear-history confirm is open')
  assert.ok(dialog.contains(document.activeElement), 'ConfirmModal autofocuses into itself')

  fireEvent(window, new window.CustomEvent('app:console:open-search'))

  assert.ok(screen.getByTestId('confirm-modal-cancel'), 'the confirm stays open')
  assert.equal(
    screen.queryByTestId('console-search-input') === null,
    true,
    'the search bar never opens over the confirm'
  )
  assert.ok(dialog.contains(document.activeElement), 'focus stays inside the aria-modal dialog')
})

test('search matches whitespace-collapsed rendered text, not raw markdown spacing', () => {
  // Agent prose arrives CLI-wrapped: soft line breaks and runs of spaces that
  // normal-flow markdown renders as single spaces. The phrase the reader sees
  // and selects must match, and the mark wraps the raw span it renders from.
  const messages = [
    makeMessage({ id: 'm1', content: 'The parser  was   correct overall' }),
    makeMessage({ id: 'm2', content: 'parser\nwas correct too', iteration: 2 })
  ]
  render(
    <I18nextProvider i18n={i18n}>
      <PairConsole pair={makePair(messages)} />
    </I18nextProvider>
  )
  fireEvent.click(screen.getByTestId('console-search-toggle'))
  const input = screen.getByTestId('console-search-input')
  fireEvent.change(input, { target: { value: 'parser was correct' } })

  assert.equal(screen.getByTestId('console-search-count').textContent, '2 matches')
  assert.ok(document.getElementById('console-msg-m1'))
  assert.ok(document.getElementById('console-msg-m2'))
  const marks = document.querySelectorAll('mark')
  assert.equal(marks.length, 2)
  assert.equal(marks[0].textContent, 'parser  was   correct')
  assert.equal(marks[1].textContent, 'parser\nwas correct')
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

// The deferred search list lags the typed query by one commit. fireEvent
// wraps every event in act, which flushes the deferred value and hides that
// lag — so these tests drive a concurrent root with native events instead:
// discrete events (input/keydown) commit their urgent update in a microtask
// after dispatch, the deferred re-render only lands on a later scheduler
// task, and waiting a single microtask leaves the stale list in place
// exactly like a fast real keystroke.
//
// Two rules keep that honest on a loaded shared runner:
// - Every parked press is preceded by a DOM check that a match of the interim
//   query only is still listed, proving the press landed on the lagging list;
//   a harness that ever flushes the deferred value inside the urgent microtask
//   fails there instead of exercising the caught-up path for the wrong reason.
// - The catch-up is awaited with waitFor, never a fixed delay: the commit that
//   outlines the card and the passive effect that records its scroll are
//   separate scheduler tasks, and CI showed a 10ms timer can expire between
//   them while the outline had already landed.
const settleUrgent = async (): Promise<void> => {
  await Promise.resolve()
}

function typeNative(input: HTMLInputElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
  setter?.call(input, value)
  input.dispatchEvent(new window.Event('input', { bubbles: true }))
}

function keyDownNative(el: Element, init: { key: string; shiftKey?: boolean }): void {
  el.dispatchEvent(new window.KeyboardEvent('keydown', { bubbles: true, ...init }))
}

async function withConcurrentConsole(
  messages: Message[],
  run: (openSearch: () => Promise<HTMLInputElement>) => Promise<void>,
  opts?: { pair?: Pair }
): Promise<void> {
  const actEnv = (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT
  ;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = false
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root: Root = createRoot(container)
  const pair = opts?.pair ?? makePair(messages)
  const openSearch = async (): Promise<HTMLInputElement> => {
    container.querySelector<HTMLButtonElement>('[data-testid="console-search-toggle"]')?.click()
    await settleUrgent()
    const input = container.querySelector<HTMLInputElement>('[data-testid="console-search-input"]')
    assert.ok(input, 'search bar rendered after the toggle click')
    return input
  }
  try {
    root.render(
      <I18nextProvider i18n={i18n}>
        <PairConsole pair={pair} />
      </I18nextProvider>
    )
    await waitFor(() => assert.ok(container.querySelector('[data-testid="console-search-toggle"]')))
    await run(openSearch)
  } finally {
    root.unmount()
    container.remove()
    ;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT =
      actEnv ?? true
  }
}

test('Enter pressed while the deferred list lags applies to the final query, not the stale one', async () => {
  scrollTargets.length = 0
  // m1 matches the 'pars' prefix but not the final 'parser' query.
  const messages = [
    makeMessage({ id: 'm1', content: 'parse tree notes', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'e1', content: 'parser fixed', from: 'executor' })
  ]
  await withConcurrentConsole(messages, async (openSearch) => {
    const input = await openSearch()
    typeNative(input, 'pars')
    await waitFor(() =>
      assert.equal(
        document.querySelector('[data-testid="console-search-count"]')?.textContent,
        '2 matches',
        'interim query committed'
      )
    )

    typeNative(input, 'parser')
    // Urgent commit only: the deferred list still holds the 'pars' matches.
    await settleUrgent()
    assert.ok(
      document.getElementById('console-msg-m1'),
      'the deferred list lags the query at keypress time'
    )
    keyDownNative(input, { key: 'Enter' })
    await settleUrgent()

    assert.equal(
      document.getElementById('console-msg-m1')?.className.includes('outline'),
      false,
      'Enter never activates a match of the stale list'
    )
    assert.equal(scrollTargets.length, 0, 'nothing scrolls off the stale list')

    await waitFor(() => {
      assert.ok(
        document.getElementById('console-msg-e1')?.className.includes('outline'),
        'the final query first match is outlined once the list catches up'
      )
      assert.equal(scrollTargets.length, 1, 'one scroll once the list catches up')
    })
    assert.equal(document.getElementById('console-msg-m1'), null, 'stale match filtered out')
    assert.ok(scrollTargets[0].closest('#console-msg-e1'), 'scroll aims inside the active card')
  })
})

test('a parked Shift+Enter activates the last match of the final list', async () => {
  scrollTargets.length = 0
  // m0 matches the 'pars' prefix but not the final 'parser' query: it makes
  // the lag observable in the DOM, since e1 and e2 match both queries.
  const messages = [
    makeMessage({ id: 'm0', content: 'parse tree notes', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'e1', content: 'parser fixed', from: 'executor' }),
    makeMessage({ id: 'e2', content: 'parser verified', from: 'executor', iteration: 2 })
  ]
  await withConcurrentConsole(messages, async (openSearch) => {
    const input = await openSearch()
    typeNative(input, 'pars')
    await waitFor(() =>
      assert.equal(
        document.querySelector('[data-testid="console-search-count"]')?.textContent,
        '3 matches',
        'interim query committed'
      )
    )

    typeNative(input, 'parser')
    await settleUrgent()
    assert.ok(
      document.getElementById('console-msg-m0'),
      'the deferred list lags the query at keypress time'
    )
    keyDownNative(input, { key: 'Enter', shiftKey: true })
    await settleUrgent()

    for (const id of ['m0', 'e1', 'e2']) {
      assert.equal(
        document.getElementById(`console-msg-${id}`)?.className.includes('outline'),
        false,
        `${id} not outlined while the press is parked`
      )
    }

    await waitFor(() => {
      assert.ok(
        document.getElementById('console-msg-e2')?.className.includes('outline'),
        'parked Shift+Enter lands on the last match of the final list'
      )
      assert.ok(scrollTargets[0]?.closest('#console-msg-e2'), 'scroll aims inside the active card')
    })
    assert.equal(
      document.getElementById('console-msg-e1')?.className.includes('outline'),
      false,
      'the first match stays inactive'
    )
    assert.equal(document.getElementById('console-msg-m0'), null, 'stale match filtered out')
  })
})

test('a parked press on a query with no final matches activates nothing', async () => {
  scrollTargets.length = 0
  // 'parse' matches m1; the final query matches nothing. The press parks on
  // the lagging list and must resolve against the empty final list rather
  // than activate anything or scroll.
  const messages = [
    makeMessage({ id: 'm1', content: 'parse tree notes', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'e1', content: 'parser fixed', from: 'executor' })
  ]
  await withConcurrentConsole(messages, async (openSearch) => {
    const input = await openSearch()
    typeNative(input, 'parse')
    await waitFor(() =>
      assert.equal(
        document.querySelector('[data-testid="console-search-count"]')?.textContent,
        '2 matches',
        'interim query committed'
      )
    )

    typeNative(input, 'parser none')
    await settleUrgent()
    assert.ok(
      document.getElementById('console-msg-m1'),
      'the deferred list lags the query at keypress time'
    )
    keyDownNative(input, { key: 'Enter' })
    await settleUrgent()

    await waitFor(() =>
      assert.equal(
        document.querySelector('[data-testid="console-search-count"]')?.textContent,
        'No matches'
      )
    )
    for (const id of ['m1', 'e1']) {
      assert.equal(
        document.getElementById(`console-msg-${id}`),
        null,
        `${id} filtered out entirely with no matches`
      )
    }
    assert.equal(scrollTargets.length, 0, 'a parked press on no matches never scrolls')
  })
})

test('two cycle presses parked during one lag compose, not overwrite', async () => {
  scrollTargets.length = 0
  // Final 'parser' list is [e1, e2]; two Enters must skip the first hit and
  // land on e2, exactly as the same two presses after catch-up would.
  const messages = [
    makeMessage({ id: 'm0', content: 'parse tree notes', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'e1', content: 'parser one', from: 'executor' }),
    makeMessage({ id: 'e2', content: 'parser two', from: 'executor', iteration: 2 })
  ]
  await withConcurrentConsole(messages, async (openSearch) => {
    const input = await openSearch()
    typeNative(input, 'pars')
    await waitFor(() =>
      assert.equal(
        document.querySelector('[data-testid="console-search-count"]')?.textContent,
        '3 matches',
        'interim query committed'
      )
    )

    typeNative(input, 'parser')
    await settleUrgent()
    assert.ok(
      document.getElementById('console-msg-m0'),
      'the deferred list lags the query at keypress time'
    )
    keyDownNative(input, { key: 'Enter' })
    keyDownNative(input, { key: 'Enter' })
    await settleUrgent()

    await waitFor(() => {
      assert.ok(
        document.getElementById('console-msg-e2')?.className.includes('outline'),
        'two parked Enters land on the second match of the final list'
      )
      assert.equal(
        document.getElementById('console-msg-e1')?.className.includes('outline'),
        false,
        'the first match was skipped, as the reader pressed twice'
      )
      assert.equal(scrollTargets.length, 1, 'one scroll to the final activation')
    })
    assert.ok(scrollTargets[0].closest('#console-msg-e2'), 'scroll aims inside the active card')
  })
})

test('a parked cycle press dies with a run switch, never applied to the new run', async () => {
  scrollTargets.length = 0
  // The archived run's a1 matches 'parser' — the parked press must never
  // reach it: no press was made in that run.
  const liveMessages = [
    makeMessage({ id: 'm1', content: 'parse tree notes', from: 'mentor', type: 'plan' }),
    makeMessage({ id: 'e1', content: 'parser fixed', from: 'executor' })
  ]
  const archivedRun: PairRunSummary = {
    id: 'run-arch',
    spec: 'archived task',
    status: 'Finished',
    startedAt: 1000,
    finishedAt: 2000,
    mentorModel: 'grok:fast',
    executorModel: 'grok:fast',
    iterations: 1,
    messages: [makeMessage({ id: 'a1', content: 'parser archived detail', from: 'executor' })]
  }
  await withConcurrentConsole(
    liveMessages,
    async (openSearch) => {
      const input = await openSearch()
      typeNative(input, 'pars')
      await waitFor(() =>
        assert.equal(
          document.querySelector('[data-testid="console-search-count"]')?.textContent,
          '2 matches',
          'interim query committed'
        )
      )

      typeNative(input, 'parser')
      await settleUrgent()
      assert.ok(
        document.getElementById('console-msg-m1'),
        'the deferred list lags the query at keypress time'
      )
      keyDownNative(input, { key: 'Enter' })
      await settleUrgent()

      // Switch to the archived run while the deferred list still lags.
      usePairStore.getState().setViewingRunId('run-arch')

      await waitFor(() => {
        assert.ok(document.getElementById('console-msg-a1'), 'archived run rendered')
        assert.equal(
          document.querySelector('[data-testid="console-search-count"]')?.textContent,
          '1 match',
          'deferred query caught up inside the archived run'
        )
      })
      assert.equal(
        document.getElementById('console-msg-a1')?.className.includes('outline'),
        false,
        'the parked press never activates a match of the run it was not pressed in'
      )
      assert.equal(scrollTargets.length, 0, 'nothing scrolls into the archived run')

      // Cycling in the archived run restarts from its own first match.
      keyDownNative(input, { key: 'Enter' })
      await waitFor(() => {
        assert.ok(
          document.getElementById('console-msg-a1')?.className.includes('outline'),
          'a fresh press in the archived run activates its match'
        )
        assert.equal(scrollTargets.length, 1)
      })
    },
    { pair: makePairWithRuns(liveMessages, archivedRun) }
  )

  usePairStore.getState().setViewingRunId(null)
})

test('a parked cycle press dies with a role-filter switch, never applied to the filtered list', async () => {
  scrollTargets.length = 0
  // Under "all" the 'parser' list is [m1, e1]; the parked press was made
  // against it and must not re-target whatever the mentor filter leaves.
  const messages = [
    makeMessage({ id: 'm1', content: 'parser plan', from: 'mentor', type: 'plan' }),
    makeMessage({
      id: 'm0',
      content: 'parse tree notes',
      from: 'mentor',
      type: 'plan',
      iteration: 2
    })
  ]
  await withConcurrentConsole(messages, async (openSearch) => {
    const input = await openSearch()
    typeNative(input, 'pars')
    await waitFor(() =>
      assert.equal(
        document.querySelector('[data-testid="console-search-count"]')?.textContent,
        '2 matches',
        'interim query committed'
      )
    )

    typeNative(input, 'parser')
    await settleUrgent()
    assert.ok(
      document.getElementById('console-msg-m0'),
      'the deferred list lags the query at keypress time'
    )
    keyDownNative(input, { key: 'Enter' })
    await settleUrgent()

    document.querySelector<HTMLButtonElement>('[data-testid="filter-mentor"]')?.click()

    await waitFor(() =>
      assert.equal(
        document.querySelector('[data-testid="console-search-count"]')?.textContent,
        '1 match',
        'deferred query caught up under the mentor filter'
      )
    )
    assert.equal(
      document.getElementById('console-msg-m1')?.className.includes('outline'),
      false,
      'the parked press never activates a match of the filtered list'
    )
    assert.equal(scrollTargets.length, 0, 'nothing scrolls after the filter switch')

    keyDownNative(input, { key: 'Enter' })
    await waitFor(() => {
      assert.ok(
        document.getElementById('console-msg-m1')?.className.includes('outline'),
        'a fresh press under the filter activates its match'
      )
      assert.equal(scrollTargets.length, 1)
    })
  })
})
