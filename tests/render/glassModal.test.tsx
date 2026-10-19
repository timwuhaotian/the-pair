/**
 * Component tests for GlassModal's keyboard handling: focus moves in on open
 * and back out on close, Tab and Shift+Tab wrap inside the dialog, and Escape
 * (or the backdrop) closes it only while it is dismissible.
 */
import './setup-dom.ts'

import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { GlassModal } from '../../src/renderer/src/components/ui/GlassModal.tsx'

afterEach(cleanup)

function Body(): React.ReactNode {
  return (
    <>
      <button type="button">first</button>
      <button type="button" disabled>
        skipped
      </button>
      <button type="button">last</button>
    </>
  )
}

function renderModal(
  props: { dismissible?: boolean; title?: string; onClose?: () => void } = {},
  children: React.ReactNode = <Body />
): ReturnType<typeof render> {
  return render(
    <>
      <button type="button">outside</button>
      <GlassModal
        isOpen
        onClose={props.onClose ?? (() => undefined)}
        dismissible={props.dismissible}
        title={props.title}
      >
        {children}
      </GlassModal>
    </>
  )
}

function pressTab(shiftKey = false): KeyboardEvent {
  const event = new window.KeyboardEvent('keydown', {
    key: 'Tab',
    shiftKey,
    bubbles: true,
    cancelable: true
  })
  fireEvent(document, event)
  return event
}

function pressEscape(): void {
  fireEvent.keyDown(document, { key: 'Escape' })
}

// Boolean checks: a failing equal() on a jsdom node inspects it for minutes.
function isFocused(name: string): boolean {
  return document.activeElement === screen.getByRole('button', { name })
}

test('opening focuses the first control and closing restores the previous focus', () => {
  const tree = (isOpen: boolean): React.ReactNode => (
    <>
      <button type="button">outside</button>
      <GlassModal isOpen={isOpen} onClose={() => undefined}>
        <Body />
      </GlassModal>
    </>
  )
  const { rerender } = render(tree(false))
  screen.getByRole('button', { name: 'outside' }).focus()
  rerender(tree(true))
  assert.ok(isFocused('first'), 'focus moved into the dialog')
  rerender(tree(false))
  assert.ok(isFocused('outside'), 'focus returned to the opener')
})

test('Tab wraps from the last control to the first, Shift+Tab the other way', () => {
  renderModal()
  screen.getByRole('button', { name: 'last' }).focus()
  assert.ok(pressTab().defaultPrevented)
  assert.ok(isFocused('first'), 'Tab wrapped to first')

  assert.ok(pressTab(true).defaultPrevented)
  assert.ok(isFocused('last'), 'Shift+Tab wrapped to last')
})

test('Tab between inner controls is left to the browser', () => {
  renderModal()
  screen.getByRole('button', { name: 'first' }).focus()
  assert.ok(!pressTab().defaultPrevented)
  screen.getByRole('button', { name: 'last' }).focus()
  assert.ok(!pressTab(true).defaultPrevented)
})

test('focus that escaped the dialog is pulled back in on Tab', () => {
  renderModal()
  screen.getByRole('button', { name: 'outside' }).focus()
  pressTab()
  assert.ok(isFocused('first'), 'Tab pulled focus to first')

  screen.getByRole('button', { name: 'outside' }).focus()
  pressTab(true)
  assert.ok(isFocused('last'), 'Shift+Tab pulled focus to last')
})

test('a dialog with nothing focusable keeps focus on itself', () => {
  renderModal({}, <p>Nothing to press here</p>)
  const dialog = screen.getByRole('dialog')
  assert.ok(document.activeElement === dialog, 'dialog focused on open')
  assert.ok(pressTab().defaultPrevented)
  assert.ok(document.activeElement === dialog, 'dialog still focused')
})

test('Escape and the backdrop close a dismissible dialog', () => {
  let closes = 0
  renderModal({ onClose: () => closes++ })
  pressEscape()
  assert.equal(closes, 1)
  const backdrop = screen.getByRole('dialog').previousElementSibling
  assert.ok(backdrop, 'backdrop rendered')
  fireEvent.click(backdrop)
  assert.equal(closes, 2)
})

test('a non-dismissible dialog ignores Escape, the backdrop and its close button', () => {
  let closes = 0
  renderModal({ dismissible: false, title: 'Installing', onClose: () => closes++ })
  pressEscape()
  const backdrop = screen.getByRole('dialog').previousElementSibling
  assert.ok(backdrop, 'backdrop rendered')
  fireEvent.click(backdrop)
  fireEvent.click(screen.getByRole('button', { name: 'close' }))
  assert.equal(closes, 0)
})
