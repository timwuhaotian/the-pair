import './setup-dom.ts'
import '../../src/renderer/src/i18n.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React, { useState } from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { ModelPicker } from '../../src/renderer/src/components/ModelPicker.tsx'
import type { AvailableModel, ProviderKind } from '../../src/renderer/src/types.ts'

function model(
  provider: ProviderKind,
  providerLabel: string,
  modelId: string,
  displayName: string,
  canonicalKey: string
): AvailableModel {
  return {
    provider,
    modelId,
    displayName,
    available: true,
    providerLabel,
    sourceProviderLabel: '',
    billingKind: 'plan',
    billingLabel: '',
    accessLabel: `${providerLabel} login`,
    availabilityStatus: 'ready',
    supportsPairExecution: true,
    recommendedRoles: ['mentor', 'executor'],
    canonicalKey
  }
}

const sonnet = model('claude', 'Claude Code', 'sonnet', 'Claude Sonnet', 'anthropic::sonnet')
const opus = model('claude', 'Claude Code', 'opus', 'Claude Opus', 'anthropic::opus')
const gpt = model('codex', 'Codex', 'gpt-5', 'GPT-5', 'openai::gpt-5')
const gptMini = model('codex', 'Codex', 'gpt-5-mini', 'GPT-5 Mini', 'openai::gpt-5-mini')
const models = [sonnet, opus, gpt, gptMini]

function Harness({ initial, onChange }: { initial: string; onChange: (id: string) => void }) {
  const [value, setValue] = useState(initial)
  return (
    <ModelPicker
      value={value}
      models={models}
      role="mentor"
      onChange={(id) => {
        onChange(id)
        setValue(id)
      }}
    />
  )
}

afterEach(() => {
  cleanup()
  localStorage.clear()
})

test('step one follows the selection; step two lists only that agent, with vendor marks', () => {
  render(<Harness initial="claude/opus" onChange={() => {}} />)
  const agents = screen.getAllByRole('radio')
  assert.deepEqual(
    agents.map((radio) => radio.getAttribute('aria-label')),
    ['Claude Code', 'Codex']
  )
  assert.equal(
    screen.getByRole('radio', { name: 'Claude Code' }).getAttribute('aria-checked'),
    'true'
  )

  fireEvent.click(screen.getByRole('button', { name: /Claude Opus/ }))
  const listbox = screen.getByRole('listbox')
  const names = within(listbox)
    .getAllByRole('option')
    .map((option) => option.title)
  assert.deepEqual(names.sort(), ['Claude Opus', 'Claude Sonnet'])
  // Every row is prefixed with its vendor's mark.
  for (const option of within(listbox).getAllByRole('option')) {
    assert.ok(option.querySelector('svg'), `${option.title} has a vendor icon`)
  }
})

test('switching agent applies its first choice and opens its model list', () => {
  const changes: string[] = []
  render(<Harness initial="claude/sonnet" onChange={(id) => changes.push(id)} />)
  fireEvent.click(screen.getByRole('radio', { name: 'Codex' }))
  assert.deepEqual(changes, ['codex/gpt-5'])
  assert.equal(screen.getByRole('radio', { name: 'Codex' }).getAttribute('aria-checked'), 'true')
  const names = within(screen.getByRole('listbox'))
    .getAllByRole('option')
    .map((option) => option.title)
  assert.deepEqual(names.sort(), ['GPT-5', 'GPT-5 Mini'])
  assert.equal(localStorage.getItem('the-pair-preferred-mentor-model'), 'codex/gpt-5')
})

test('switching agent restores the most recent model used with it for this role', () => {
  localStorage.setItem(
    'the-pair-recent-models-mentor',
    JSON.stringify(['claude/sonnet', 'codex/gpt-5-mini'])
  )
  const changes: string[] = []
  render(<Harness initial="claude/sonnet" onChange={(id) => changes.push(id)} />)
  fireEvent.click(screen.getByRole('radio', { name: 'Codex' }))
  assert.deepEqual(changes, ['codex/gpt-5-mini'])
})
