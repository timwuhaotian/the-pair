/**
 * Component unit tests for SkillPicker: the localized labels of the trigger,
 * the search field and the refresh button plus the discovery / empty states.
 */
import './setup-dom.ts'
import './mock-ipc.ts'
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { mockIPC } from '@tauri-apps/api/mocks'
import { I18nextProvider } from 'react-i18next'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import { SkillPicker } from '../../src/renderer/src/components/SkillPicker.tsx'
import { GlassModal } from '../../src/renderer/src/components/ui/GlassModal.tsx'

interface SkillRow {
  name: string
  description: string
  source: string
}

let discovered: SkillRow[] = []
let discoveryPending = false

mockIPC((cmd) => {
  if (cmd === 'discover_skills' || cmd === 'skill_discover') {
    if (discoveryPending) return new Promise<never>(() => undefined)
    return discovered
  }
  return undefined
})

function setLocale(): void {
  void i18n.changeLanguage('en')
}

function wrap(node: React.ReactNode): React.ReactNode {
  return <I18nextProvider i18n={i18n}>{node}</I18nextProvider>
}

afterEach(() => {
  cleanup()
  discovered = []
  discoveryPending = false
})

test('SkillPicker: opens a dropdown with a search field and a refresh button', async () => {
  setLocale()
  render(wrap(<SkillPicker projectDir="/repo" onSelect={() => undefined} />))

  fireEvent.click(screen.getByRole('button', { name: 'Add skill' }))

  assert.ok(await screen.findByPlaceholderText('Search skills…'))
  assert.ok(screen.getByRole('button', { name: 'refresh' }))
})

test('SkillPicker: announces the discovery while the skills load', async () => {
  setLocale()
  discoveryPending = true
  render(wrap(<SkillPicker projectDir="/repo" onSelect={() => undefined} />))

  fireEvent.click(screen.getByRole('button', { name: 'Add skill' }))

  assert.ok(await screen.findByText('Discovering skills…'))
})

test('SkillPicker: reports an empty project', async () => {
  setLocale()
  discovered = []
  render(wrap(<SkillPicker projectDir="/repo" onSelect={() => undefined} />))

  fireEvent.click(screen.getByRole('button', { name: 'Add skill' }))

  await screen.findByText('— no skills found —')
})

test('SkillPicker: lists the discovered skills and picks one', async () => {
  setLocale()
  discovered = [{ name: 'code-review', description: 'Reviews a diff', source: 'project' }]
  const picked: string[] = []
  render(wrap(<SkillPicker projectDir="/repo" onSelect={(name) => picked.push(name)} />))

  fireEvent.click(screen.getByRole('button', { name: 'Add skill' }))
  await screen.findByText('code-review')
  assert.ok(screen.getByText('Reviews a diff'))

  fireEvent.click(screen.getByRole('option', { name: /code-review/ }))
  await waitFor(() => assert.deepEqual(picked, ['code-review']))
})

test('SkillPicker: reports that the filter matches no skill', async () => {
  setLocale()
  discovered = [{ name: 'code-review', description: 'Reviews a diff', source: 'project' }]
  render(wrap(<SkillPicker projectDir="/repo" onSelect={() => undefined} />))

  fireEvent.click(screen.getByRole('button', { name: 'Add skill' }))
  const input = await screen.findByPlaceholderText('Search skills…')
  fireEvent.change(input, { target: { value: 'nothing-matches' } })

  await screen.findByText('— no skills match —')
})

const THREE_SKILLS: SkillRow[] = [
  { name: 'alpha', description: 'First', source: 'project' },
  { name: 'beta', description: 'Second', source: 'project' },
  { name: 'gamma', description: 'Third', source: 'project' }
]

function pressTabOn(target: Element, shiftKey = false): KeyboardEvent {
  const event = new window.KeyboardEvent('keydown', {
    key: 'Tab',
    shiftKey,
    bubbles: true,
    cancelable: true
  })
  fireEvent(target, event)
  return event
}

test('SkillPicker: the search field is a combobox tracking the highlighted skill', async () => {
  setLocale()
  discovered = THREE_SKILLS
  const picked: string[] = []
  render(wrap(<SkillPicker projectDir="/repo" onSelect={(name) => picked.push(name)} />))

  const trigger = screen.getByRole('button', { name: 'Add skill' })
  fireEvent.click(trigger)
  await screen.findByText('alpha')
  const input = screen.getByRole('combobox', { name: 'Search skills…' })
  const listbox = screen.getByRole('listbox')
  assert.equal(input.getAttribute('aria-controls'), listbox.id)
  const options = screen.getAllByRole('option')
  assert.equal(input.getAttribute('aria-activedescendant'), options[0].id)

  fireEvent.keyDown(input, { key: 'ArrowDown' })
  assert.equal(input.getAttribute('aria-activedescendant'), options[1].id)
  fireEvent.keyDown(input, { key: 'End' })
  assert.equal(input.getAttribute('aria-activedescendant'), options[2].id)
  fireEvent.keyDown(input, { key: 'Home' })
  assert.equal(input.getAttribute('aria-activedescendant'), options[0].id)
  fireEvent.keyDown(input, { key: 'ArrowDown' })

  input.focus()
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.deepEqual(picked, ['beta'])
  assert.ok(!screen.queryByRole('listbox'), 'menu closed')
  assert.ok(document.activeElement === trigger, 'focus went back to the trigger')
})

test('SkillPicker: inside a dialog, Tab moves within the portaled menu and out via the trigger', async () => {
  setLocale()
  discovered = THREE_SKILLS
  render(
    wrap(
      <GlassModal isOpen onClose={() => undefined} title="Assign task">
        <SkillPicker projectDir="/repo" onSelect={() => undefined} />
        <button type="button">after</button>
      </GlassModal>
    )
  )

  const trigger = screen.getByRole('button', { name: 'Add skill' })
  fireEvent.click(trigger)
  await screen.findByText('alpha')
  const input = screen.getByRole('combobox', { name: 'Search skills…' })
  input.focus()

  // The menu lives outside the dialog's DOM; the dialog trap must not yank
  // focus out of it.
  const tab = pressTabOn(input)
  assert.ok(!tab.defaultPrevented, 'Tab inside the menu is left to the browser')
  assert.ok(document.activeElement === input, 'focus was not pulled back into the dialog')

  // Tab past the menu's last control closes it and continues from the trigger.
  const refresh = screen.getByRole('button', { name: 'refresh' })
  refresh.focus()
  pressTabOn(refresh)
  assert.ok(!screen.queryByRole('listbox'), 'menu closed')
  assert.ok(document.activeElement === trigger, 'focus is back on the trigger')
})
