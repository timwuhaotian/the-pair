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
