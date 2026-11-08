/**
 * Component unit tests for the command palette:
 *   - Filters actions by fuzzy query and groups them.
 *   - Enter runs the active action; Esc closes.
 *   - Unavailable actions are hidden.
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
import { CommandPalette } from '../../src/renderer/src/components/CommandPalette.tsx'
import type { PaletteAction } from '../../src/renderer/src/lib/paletteActions.ts'
import { assertAbsent } from './assert-dom.ts'

function setLocale(): void {
  void i18n.changeLanguage('en')
}

function makeActions(calls: string[]): PaletteAction[] {
  return [
    {
      id: 'pair.new',
      labelKey: 'palette.newPair',
      groupKey: 'palette.groups.pair',
      run: () => calls.push('new')
    },
    {
      id: 'nav.activity',
      labelKey: 'palette.goActivity',
      groupKey: 'palette.groups.go',
      run: () => calls.push('activity')
    },
    {
      id: 'pair.pause',
      labelKey: 'palette.pausePair',
      groupKey: 'palette.groups.pair',
      available: () => false,
      run: () => calls.push('pause')
    }
  ]
}

function renderPalette(actions: PaletteAction[], onClose: () => void = () => undefined): void {
  render(
    <I18nextProvider i18n={i18n}>
      <CommandPalette isOpen onClose={onClose} actions={actions} />
    </I18nextProvider>
  )
}

afterEach(() => {
  cleanup()
})

test('Palette: lists available actions under group headers', () => {
  setLocale()
  renderPalette(makeActions([]))

  assert.ok(screen.getByTestId('command-palette'))
  assert.ok(screen.getByTestId('palette-item-pair.new'))
  assert.ok(screen.getByTestId('palette-item-nav.activity'))
  // Gated actions never render.
  assertAbsent(screen.queryByTestId('palette-item-pair.pause'))
})

test('Palette: typing filters to matches, Enter runs the top hit', () => {
  setLocale()
  const calls: string[] = []
  let closed = 0
  renderPalette(makeActions(calls), () => {
    closed += 1
  })

  fireEvent.change(screen.getByTestId('command-palette-input'), { target: { value: 'activ' } })
  assertAbsent(screen.queryByTestId('palette-item-pair.new'))
  assert.ok(screen.getByTestId('palette-item-nav.activity'))

  fireEvent.keyDown(screen.getByTestId('command-palette-input'), { key: 'Enter' })
  assert.deepEqual(calls, ['activity'])
  assert.equal(closed, 1)
})

test('Palette: no matches render the empty state', () => {
  setLocale()
  renderPalette(makeActions([]))

  fireEvent.change(screen.getByTestId('command-palette-input'), { target: { value: 'zzz-nope' } })
  assertAbsent(screen.queryByTestId('palette-item-pair.new'))
  assert.ok(screen.getByText(/No matching commands/))
})

test('Palette: Escape closes without running anything', () => {
  setLocale()
  const calls: string[] = []
  let closed = 0
  renderPalette(makeActions(calls), () => {
    closed += 1
  })

  fireEvent.keyDown(screen.getByTestId('command-palette-input'), { key: 'Escape' })
  assert.deepEqual(calls, [])
  assert.equal(closed, 1)
})

test('Palette: arrow keys move the active item before Enter', () => {
  setLocale()
  const calls: string[] = []
  renderPalette(makeActions(calls))

  const input = screen.getByTestId('command-palette-input')
  fireEvent.keyDown(input, { key: 'ArrowDown' })
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.deepEqual(calls, ['activity'])
})
