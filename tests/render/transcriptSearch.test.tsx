/**
 * Component tests for transcript search highlighting:
 *   - Matched substrings render as <mark> inside markdown prose.
 *   - No query renders prose untouched (no marks).
 *   - Active-match message card carries the console-msg anchor + outline class.
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
import { MarkdownContent } from '../../src/renderer/src/components/MarkdownContent.tsx'
import { MessageCard } from '../../src/renderer/src/components/MessageCard.tsx'
import { countSearchHighlights } from '../../src/renderer/src/lib/searchableCard.ts'
import type { Message } from '../../src/renderer/src/store/usePairStore.ts'

afterEach(() => {
  cleanup()
})

test('MarkdownContent wraps query matches in <mark>', () => {
  const { container } = render(
    <MarkdownContent content="Fix the Parser bug, parser fans" searchQuery="parser" />
  )
  const marks = container.querySelectorAll('mark')
  assert.equal(marks.length, 2)
  assert.equal(marks[0]?.textContent, 'Parser')
  assert.equal(marks[1]?.textContent, 'parser')
  assert.ok(container.textContent?.includes('Fix the'))
})

test('MarkdownContent highlights matches inside inline code', () => {
  const { container } = render(
    <MarkdownContent content="Edited `src/parser.ts` and ran tests" searchQuery="parser.ts" />
  )
  const marks = container.querySelectorAll('mark')
  assert.equal(marks.length, 1)
  assert.equal(marks[0]?.textContent, 'parser.ts')
  assert.ok(marks[0]?.closest('code'), 'mark stays inside the code element')
})

test('MarkdownContent highlights matches inside fenced code blocks', () => {
  const { container } = render(
    <MarkdownContent content={'```\nparser failed\n```'} searchQuery="parser" />
  )
  assert.equal(container.querySelectorAll('mark').length, 1)
})

test('MarkdownContent without a query renders no marks', () => {
  const { container } = render(<MarkdownContent content="Fix the Parser bug" />)
  assert.equal(container.querySelectorAll('mark').length, 0)
})

test('MarkdownContent with an empty query renders no marks', () => {
  const { container } = render(<MarkdownContent content="Fix the Parser bug" searchQuery="  " />)
  assert.equal(container.querySelectorAll('mark').length, 0)
})

const baseMessage: Message = {
  id: 'msg-1',
  timestamp: 1000,
  from: 'executor',
  to: 'human',
  type: 'result',
  content: 'Parser fixed and parser tests pass',
  iteration: 1
}

test('MessageCard anchors at console-msg-<id> and outlines the active match', () => {
  void i18n.changeLanguage('en')
  const { container } = render(
    <I18nextProvider i18n={i18n}>
      <MessageCard msg={baseMessage} searchQuery="parser" isActiveMatch />
    </I18nextProvider>
  )
  const anchor = container.querySelector('#console-msg-msg-1')
  assert.ok(anchor, 'anchor element exists')
  assert.ok(anchor.className.includes('outline'), 'active outline class present')
  assert.equal(container.querySelectorAll('mark').length, 2)
})

test('MessageCard expands a collapsed long body when it is the active match', () => {
  void i18n.changeLanguage('en')
  const longMessage: Message = {
    ...baseMessage,
    id: 'msg-long',
    content: `parser intro ${'x'.repeat(1300)} parser tail end`
  }
  const { container, rerender } = render(
    <I18nextProvider i18n={i18n}>
      <MessageCard msg={longMessage} searchQuery="parser" />
    </I18nextProvider>
  )
  const anchor = container.querySelector('#console-msg-msg-long')
  assert.ok(anchor)
  // Collapsed: the prose wrapper clips to 280px while not active.
  assert.ok(anchor.innerHTML.includes('max-h-[280px]'), 'collapsed before activation')

  rerender(
    <I18nextProvider i18n={i18n}>
      <MessageCard msg={longMessage} searchQuery="parser" isActiveMatch />
    </I18nextProvider>
  )
  assert.ok(!anchor.innerHTML.includes('max-h-[280px]'), 'expanded while active')
  assert.equal(container.querySelectorAll('mark').length, 2)
})

test('MessageCard hides the no-op collapse toggle on an active long-body match', async () => {
  await i18n.changeLanguage('en')
  // No steps: while active the clip is lifted and nothing is left to toggle.
  const longMessage: Message = {
    ...baseMessage,
    id: 'msg-long',
    content: `parser intro ${'x'.repeat(1300)} parser tail end`
  }
  const { container, rerender } = render(
    <I18nextProvider i18n={i18n}>
      <MessageCard msg={longMessage} searchQuery="parser" isActiveMatch />
    </I18nextProvider>
  )
  const anchor = container.querySelector('#console-msg-msg-long')
  assert.ok(anchor)
  assert.ok(!anchor.innerHTML.includes('max-h-[280px]'), 'body unclipped while active')
  assert.equal(container.querySelectorAll('button').length, 0, 'no toggle, no overlay')

  // Once the match moves on, the card returns to its collapsed default.
  rerender(
    <I18nextProvider i18n={i18n}>
      <MessageCard msg={longMessage} searchQuery="parser" />
    </I18nextProvider>
  )
  assert.ok(anchor.innerHTML.includes('max-h-[280px]'), 'collapsed again after deactivation')
  assert.ok(screen.getByRole('button', { name: /Show full report/ }), 'toggle restored')
})

test('MessageCard keeps the details toggle honest on an active match with steps', async () => {
  await i18n.changeLanguage('en')
  const msg: Message = {
    ...baseMessage,
    id: 'msg-steps',
    content: `parser intro ${'x'.repeat(1300)} parser tail end`,
    cognitiveEvents: [
      {
        id: 'ev1',
        timestamp: 1100,
        role: 'executor',
        eventType: 'tool_call',
        toolName: 'edit',
        description: 'edited parser.ts',
        status: 'completed'
      }
    ]
  }
  const { container, rerender } = render(
    <I18nextProvider i18n={i18n}>
      <MessageCard msg={msg} searchQuery="parser" isActiveMatch />
    </I18nextProvider>
  )
  const anchor = container.querySelector('#console-msg-msg-steps')
  assert.ok(anchor)
  assert.ok(!anchor.innerHTML.includes('max-h-[280px]'), 'active match lifts the body clip')

  // The toggle stays functional while active: it reveals the hidden steps.
  const toggle = screen.getByRole('button', { name: /show 1 step/i })
  assert.equal(toggle.getAttribute('aria-expanded'), 'false')
  fireEvent.click(toggle)
  assert.ok(screen.getByRole('button', { name: /Hide details/ }))
  assert.equal(
    screen.getByRole('button', { name: /Hide details/ }).getAttribute('aria-expanded'),
    'true'
  )
  assert.ok(anchor.textContent?.includes('edited parser.ts'), 'steps panel opened')

  // Cycling away keeps the card in the state the user chose...
  rerender(
    <I18nextProvider i18n={i18n}>
      <MessageCard msg={msg} searchQuery="parser" />
    </I18nextProvider>
  )
  assert.ok(!anchor.innerHTML.includes('max-h-[280px]'), 'stays expanded as requested')

  // ...and asking to hide collapses it again, active or not.
  fireEvent.click(screen.getByRole('button', { name: /Hide details/ }))
  assert.ok(anchor.innerHTML.includes('max-h-[280px]'), 'collapsed on request')
  rerender(
    <I18nextProvider i18n={i18n}>
      <MessageCard msg={msg} searchQuery="parser" isActiveMatch />
    </I18nextProvider>
  )
  assert.ok(!anchor.innerHTML.includes('max-h-[280px]'), 'clip lifted while active again')
  const restored = screen.getByRole('button', { name: /Show 1 step/i })
  assert.equal(restored.getAttribute('aria-expanded'), 'false', 'toggle reflects the chosen state')
})

test('MessageCard without search props has the anchor but no outline or marks', () => {
  void i18n.changeLanguage('en')
  const { container } = render(
    <I18nextProvider i18n={i18n}>
      <MessageCard msg={baseMessage} />
    </I18nextProvider>
  )
  const anchor = container.querySelector('#console-msg-msg-1')
  assert.ok(anchor, 'anchor element exists')
  assert.ok(!anchor.className.includes('outline'), 'no outline without active match')
  assert.equal(container.querySelectorAll('mark').length, 0)
})

test('MarkdownContent does not match a space query across code-block lines', () => {
  const { container } = render(
    <MarkdownContent content={'```\nstepOne()\nstepTwo()\n```'} searchQuery="stepOne() stepTwo()" />
  )
  assert.equal(container.querySelectorAll('mark').length, 0)
})

test('MarkdownContent matches code-block text with its literal whitespace', () => {
  const { container } = render(
    <MarkdownContent
      content={'```\nstepOne()\nstepTwo()\n```'}
      searchQuery={'stepOne()\nstepTwo()'}
    />
  )
  const marks = container.querySelectorAll('mark')
  assert.equal(marks.length, 1)
  assert.equal(marks[0]?.textContent, 'stepOne()\nstepTwo()')
  assert.ok(marks[0]?.closest('pre'), 'mark stays inside the code block')
})

test('MarkdownContent renders no marks for a footnote reached only via an unreferenced definition', () => {
  // The [^2] reference sits inside the [^1] definition, which never
  // renders, so [^2] never converts and its definition never lands in the
  // footer. Pins the renderer behavior searchableCard mirrors.
  const content = 'foo bar\n\n[^1]: defines one[^2]\n\n[^2]: the parsernote'
  const { container } = render(<MarkdownContent content={content} searchQuery="parsernote" />)
  assert.equal(container.querySelectorAll('mark').length, 0)
})

test('MarkdownContent renders no marks for a footnote referenced only in a dropped table cell', () => {
  const content = '| a | b |\n| --- | --- |\n| x | y | [^1] |\n\n[^1]: the parsernote'
  const { container } = render(<MarkdownContent content={content} searchQuery="parsernote" />)
  assert.equal(container.querySelectorAll('mark').length, 0)
})

test('MarkdownContent renders a footnote referenced from another rendered footnote', () => {
  // The footer loop re-scans footnoteOrder as it converts each rendered
  // definition, so references inside rendered footnotes render transitively.
  const content = 'see[^1]\n\n[^1]: then[^2]\n\n[^2]: the parsernote'
  const { container } = render(<MarkdownContent content={content} searchQuery="parsernote" />)
  assert.equal(container.querySelectorAll('mark').length, 1)
})

test('MarkdownContent renders only the first definition of a duplicate footnote label', () => {
  // footnoteById keeps the first definition per label; the duplicate never
  // lands in the footer. Pins the renderer behavior searchableCard mirrors.
  const content = 'ref[^1]\n\n[^1]: alpha note\n\n[^1]: beta note'
  const alpha = render(<MarkdownContent content={content} searchQuery="alpha" />)
  assert.equal(alpha.container.querySelectorAll('mark').length, 1)
  const beta = render(<MarkdownContent content={content} searchQuery="beta" />)
  assert.equal(beta.container.querySelectorAll('mark').length, 0)
})

test('MarkdownContent renders no marks for text matching only footnote chrome', () => {
  // The counter digit, the ↩ backref, and the sr-only "Footnotes" heading
  // are generated by mdast-util-to-hast with no mdast text node behind them,
  // so searchableCard never counts them — highlighting them would break the
  // count === <mark> invariant (a rendered match with the card unlisted).
  const content = 'see[^1] here\n\n[^1]: note'
  const { container } = render(<MarkdownContent content={content} searchQuery="1" />)
  assert.equal(container.querySelectorAll('mark').length, 0)
  const backref = render(<MarkdownContent content={content} searchQuery="↩" />)
  assert.equal(backref.container.querySelectorAll('mark').length, 0)
  const heading = render(<MarkdownContent content={content} searchQuery="Footnotes" />)
  assert.equal(heading.container.querySelectorAll('mark').length, 0)
})

test('MarkdownContent keeps the footnote chrome exclusion invariant with the search count', () => {
  // Same message, same query: the rendered <mark> count must equal
  // countSearchHighlights — including the chrome-only query that renders no
  // highlight at all.
  const msg: Message = {
    ...baseMessage,
    id: 'msg-fn',
    content: 'see[^1] here\n\n[^1]: note'
  }
  for (const query of ['1', '↩', 'Footnotes', 'note', 'see']) {
    const { container } = render(<MarkdownContent content={msg.content} searchQuery={query} />)
    assert.equal(
      container.querySelectorAll('mark').length,
      countSearchHighlights(msg, query),
      `query ${JSON.stringify(query)}: marks and count disagree`
    )
  }
})
