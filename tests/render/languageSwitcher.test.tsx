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

function focusedItemName(): string {
  const active = document.activeElement
  return active?.getAttribute('role') === 'menuitemradio' ? (active.textContent ?? '') : ''
}

test('the toggle announces a menu, and opening focuses the language in use', () => {
  useLocaleStore.getState().setLocale('ja')
  const toggle = openMenu()
  assert.equal(toggle.getAttribute('aria-haspopup'), 'menu')
  assert.match(focusedItemName(), /日本語/)
  // Roving tabindex: only the focused item is a Tab stop.
  const tabStops = screen
    .getAllByRole('menuitemradio')
    .filter((item) => item.getAttribute('tabindex') === '0')
  assert.equal(tabStops.length, 1)
  assert.ok(tabStops[0] === document.activeElement, 'the Tab stop is the focused item')
})

test('arrow keys, Home and End move focus through the menu and wrap', () => {
  openMenu()
  const menu = screen.getByRole('menu')
  assert.match(focusedItemName(), /English/)
  fireEvent.keyDown(menu, { key: 'ArrowDown' })
  assert.match(focusedItemName(), /中文|简体/)
  fireEvent.keyDown(menu, { key: 'End' })
  assert.match(focusedItemName(), /한국어/)
  fireEvent.keyDown(menu, { key: 'ArrowDown' })
  assert.match(focusedItemName(), /English/, 'ArrowDown wraps to the first item')
  fireEvent.keyDown(menu, { key: 'ArrowUp' })
  assert.match(focusedItemName(), /한국어/, 'ArrowUp wraps to the last item')
  fireEvent.keyDown(menu, { key: 'Home' })
  assert.match(focusedItemName(), /English/)
  const tabStop = screen
    .getAllByRole('menuitemradio')
    .find((item) => item.getAttribute('tabindex') === '0')
  assert.ok(tabStop === document.activeElement, 'the Tab stop follows focus')
})

test('Tab closes the menu from the toggle; picking an item returns focus there too', () => {
  const toggle = openMenu()
  fireEvent.keyDown(screen.getByRole('menu'), { key: 'Tab' })
  assert.ok(!menuIsOpen(), 'Tab closed the menu')
  assert.ok(document.activeElement === toggle, 'focus is back on the toggle')

  fireEvent.click(toggle)
  fireEvent.click(screen.getByRole('menuitemradio', { name: /한국어/ }))
  assert.ok(!menuIsOpen(), 'picking closed the menu')
  assert.ok(document.activeElement === toggle, 'focus is back on the toggle after a pick')
})
