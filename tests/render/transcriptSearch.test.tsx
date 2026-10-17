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
import { cleanup, render } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import { MarkdownContent } from '../../src/renderer/src/components/MarkdownContent.tsx'
import { MessageCard } from '../../src/renderer/src/components/MessageCard.tsx'
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
