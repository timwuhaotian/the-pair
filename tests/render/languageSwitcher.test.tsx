/**
 * Component tests for the chrome language menu: Escape and outside clicks
 * close it, Escape hands focus back to the toggle, and the panel flips above
 * the toggle when there is no room below.
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
import { LanguageSwitcher } from '../../src/renderer/src/components/LanguageSwitcher.tsx'
import { useLocaleStore } from '../../src/renderer/src/store/useLocaleStore.ts'

const originalRect = window.Element.prototype.getBoundingClientRect

function stubToggleRect(top: number): void {
  const rect = { top, bottom: top + 28, left: 400, right: 428, width: 28, height: 28 }
  window.Element.prototype.getBoundingClientRect = () => ({
    ...rect,
    x: rect.left,
    y: rect.top,
    toJSON: () => rect
  })
}

function openMenu(): HTMLElement {
  render(
    <I18nextProvider i18n={i18n}>
      <LanguageSwitcher />
    </I18nextProvider>
  )
  const toggle = screen.getByTestId('chrome-language-toggle')
  fireEvent.click(toggle)
  assert.equal(toggle.getAttribute('aria-expanded'), 'true')
  return toggle
}

// Boolean checks: a failing equal() on a jsdom node inspects it for minutes.
function menuIsOpen(): boolean {
  return screen.queryByRole('menu') !== null
}

beforeEach(() => {
  useLocaleStore.getState().setLocale('en')
  stubToggleRect(20)
})

afterEach(() => {
  cleanup()
  window.Element.prototype.getBoundingClientRect = originalRect
  useLocaleStore.getState().setLocale('en')
})

test('Escape closes the menu, claims the key and refocuses the toggle', () => {
  const toggle = openMenu()
  const escape = new window.KeyboardEvent('keydown', {
    key: 'Escape',
    bubbles: true,
    cancelable: true
  })
  fireEvent(document, escape)
  assert.ok(!menuIsOpen(), 'menu closed')
  assert.ok(escape.defaultPrevented, 'Escape claimed so nothing else closes with it')
  assert.ok(document.activeElement === toggle, 'focus is back on the toggle')
})

test('an Escape something else already claimed leaves the menu open', () => {
  openMenu()
  const escape = new window.KeyboardEvent('keydown', {
    key: 'Escape',
    bubbles: true,
    cancelable: true
  })
  escape.preventDefault()
  fireEvent(document, escape)
  assert.ok(menuIsOpen(), 'menu stays open')
})

test('a click outside closes the menu; a click inside does not', () => {
  openMenu()
  fireEvent.mouseDown(screen.getByRole('menu'))
  assert.ok(menuIsOpen(), 'inside click keeps it open')
  fireEvent.mouseDown(document.body)
  assert.ok(!menuIsOpen(), 'outside click closes it')
})

test('picking a language switches the locale and closes the menu', () => {
  openMenu()
  const english = screen.getByRole('menuitemradio', { name: /English/ })
  assert.equal(english.getAttribute('aria-checked'), 'true')
  fireEvent.click(screen.getByRole('menuitemradio', { name: /日本語/ }))
  assert.equal(useLocaleStore.getState().locale, 'ja')
  assert.ok(!menuIsOpen(), 'menu closed')
})

test('the panel opens below the toggle, or above it when it would overflow', () => {
  openMenu()
  // 20 (top) + 28 (height) + 4 gap; right edge 428 - 180 panel width.
  assert.equal(screen.getByRole('menu').style.top, '52px')
  assert.equal(screen.getByRole('menu').style.left, '248px')
  cleanup()

  // jsdom's window is 768px tall: a toggle at 700 leaves no room for 220px.
  stubToggleRect(700)
  openMenu()
  assert.equal(screen.getByRole('menu').style.top, `${700 - 220 - 4}px`)
})
