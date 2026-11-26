/**
 * Component tests for GlassModal's keyboard handling: focus moves in on open
 * and back out on close, Tab and Shift+Tab wrap inside the dialog, and Escape
 * (or the backdrop) closes it only while it is dismissible.
 */
import './setup-dom.ts'
import i18n from '../../src/renderer/src/i18n.ts'

import React, { useState } from 'react'
import { createPortal } from 'react-dom'
import assert from 'node:assert/strict'
import test, { afterEach, beforeEach } from 'node:test'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { GlassModal } from '../../src/renderer/src/components/ui/GlassModal.tsx'
import { ConfirmModal } from '../../src/renderer/src/components/ui/ConfirmModal.tsx'
import { assertAbsent } from './assert-dom.ts'

beforeEach(() => {
  void i18n.changeLanguage('en')
})
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
        title={props.title ?? 'Test dialog'}
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
      <GlassModal isOpen={isOpen} onClose={() => undefined} title="Test dialog">
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

test('a field marked data-autofocus takes focus over the first control', () => {
  renderModal(
    {},
    <>
      <button type="button">first</button>
      <textarea aria-label="task" data-autofocus />
    </>
  )
  assert.ok(
    document.activeElement === screen.getByRole('textbox', { name: 'task' }),
    'the marked field got focus'
  )
})

test('Tab wraps from the last control to the header close button, Shift+Tab the other way', () => {
  renderModal()
  screen.getByRole('button', { name: 'last' }).focus()
  assert.ok(pressTab().defaultPrevented)
  assert.ok(isFocused('Close'), 'Tab wrapped to the header close button')

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
  assert.ok(isFocused('Close'), 'Tab pulled focus back into the dialog')

  screen.getByRole('button', { name: 'outside' }).focus()
  pressTab(true)
  assert.ok(isFocused('last'), 'Shift+Tab pulled focus to last')
})

test('a dialog whose body has nothing focusable keeps focus on itself', () => {
  renderModal({}, <p>Nothing to press here</p>)
  const dialog = screen.getByRole('dialog')
  assert.ok(document.activeElement === dialog, 'dialog focused on open')
  // Tab must not escape the dialog: it lands on the header's close button.
  assert.ok(pressTab().defaultPrevented)
  assert.ok(isFocused('Close'), 'Tab stayed inside the dialog')
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
  fireEvent.click(screen.getByRole('button', { name: 'Close' }))
  assert.equal(closes, 0)
})

test('a custom header replaces the title block and keeps the dialog named', () => {
  render(
    <GlassModal
      isOpen
      onClose={() => undefined}
      title="Create New Pair"
      header={<span data-testid="brand-header">brand</span>}
    >
      <Body />
    </GlassModal>
  )

  const dialog = screen.getByRole('dialog')
  // The title still names the dialog even though the title block is gone.
  assert.equal(dialog.getAttribute('aria-label'), 'Create New Pair')
  assert.ok(screen.getByTestId('brand-header'))
  // The default title heading is replaced, and the close button survives.
  assertAbsent(screen.queryByRole('heading', { name: 'Create New Pair' }))
  assert.ok(screen.getByRole('button', { name: 'Close' }))
})

test('the close button is named in the active locale', () => {
  void i18n.changeLanguage('ja')
  try {
    renderModal()
    assert.ok(screen.getByTestId('modal-close').getAttribute('aria-label') === '閉じる')
  } finally {
    void i18n.changeLanguage('en')
  }
})

test('unmounting an open dialog hands focus back to the opener', () => {
  // Callers that mount the modal only while it is open (or key it on the
  // open flag) never pass isOpen=false: the unmount itself must restore focus.
  function Host(): React.ReactNode {
    const [open, setOpen] = useState(false)
    return (
      <>
        <button type="button" onClick={() => setOpen(true)}>
          opener
        </button>
        {open && (
          <GlassModal isOpen onClose={() => setOpen(false)} title="Mounted while open">
            <button type="button" onClick={() => setOpen(false)}>
              done
            </button>
          </GlassModal>
        )}
      </>
    )
  }
  render(<Host />)
  const opener = screen.getByRole('button', { name: 'opener' })
  opener.focus()
  fireEvent.click(opener)
  assert.ok(isFocused('done'), 'focus moved into the dialog')
  fireEvent.click(screen.getByRole('button', { name: 'done' }))
  assertAbsent(screen.queryByRole('dialog'))
  assert.ok(document.activeElement === opener, 'focus returned to the opener after unmount')
})

test('closing does not steal focus that already moved elsewhere on purpose', () => {
  const tree = (isOpen: boolean): React.ReactNode => (
    <>
      <button type="button">outside</button>
      <button type="button">elsewhere</button>
      <GlassModal isOpen={isOpen} onClose={() => undefined} title="Test dialog">
        <Body />
      </GlassModal>
    </>
  )
  const { rerender } = render(tree(false))
  screen.getByRole('button', { name: 'outside' }).focus()
  rerender(tree(true))
  screen.getByRole('button', { name: 'elsewhere' }).focus()
  rerender(tree(false))
  assert.ok(isFocused('elsewhere'), 'focus stayed where the close action put it')
})

/** A picker-style menu portaled to <body>, outside the dialog's DOM. */
function PortaledPopover(): React.ReactNode {
  return (
    <>
      <button type="button">trigger</button>
      {createPortal(
        <div data-modal-popover>
          <input aria-label="filter" />
          <button type="button">refresh</button>
        </div>,
        document.body
      )}
    </>
  )
}

test('Tab inside a portaled picker menu is left to the menu, not yanked back', () => {
  renderModal({}, <PortaledPopover />)
  const filter = screen.getByRole('textbox', { name: 'filter' })
  filter.focus()
  const tab = pressTab()
  assert.ok(!tab.defaultPrevented, 'the dialog trap left the Tab alone')
  assert.ok(document.activeElement === filter, 'focus was not pulled to the first control')

  assert.ok(!pressTab(true).defaultPrevented, 'Shift+Tab is left alone too')
})

test('focus outside both the dialog and its menus is still pulled back in', () => {
  renderModal({}, <PortaledPopover />)
  screen.getByRole('button', { name: 'outside' }).focus()
  assert.ok(pressTab().defaultPrevented)
  assert.ok(isFocused('Close'), 'Tab pulled focus back into the dialog')
})

test('describedBy names the element that describes the dialog', () => {
  render(
    <GlassModal isOpen onClose={() => undefined} title="Described" describedBy="dialog-desc">
      <p id="dialog-desc">What this dialog is about</p>
    </GlassModal>
  )
  assert.ok(screen.getByRole('dialog').getAttribute('aria-describedby') === 'dialog-desc')
})

test('ConfirmModal describes its dialog with the confirmation message', () => {
  render(
    <ConfirmModal
      isOpen
      title="Delete pair?"
      message="This removes the pair and its history."
      onConfirm={() => undefined}
      onCancel={() => undefined}
    />
  )
  const describedBy = screen.getByRole('dialog').getAttribute('aria-describedby')
  assert.ok(describedBy, 'dialog has aria-describedby')
  const description = document.getElementById(describedBy)
  assert.ok(description?.textContent === 'This removes the pair and its history.')
})
