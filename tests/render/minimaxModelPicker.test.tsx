import './setup-dom.ts'
import '../../src/renderer/src/i18n.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { ModelPicker } from '../../src/renderer/src/components/ModelPicker.tsx'
import type { AvailableModel } from '../../src/renderer/src/types.ts'

function model(modelId: string, displayName: string): AvailableModel {
  return {
    provider: 'opencode',
    modelId,
    displayName,
    available: true,
    providerLabel: 'OpenCode',
    sourceProviderLabel: 'MiniMax',
    billingKind: 'byok',
    billingLabel: 'Pay as you go',
    accessLabel: 'API key',
    planLabel: 'provider-backed',
    availabilityStatus: 'ready',
    supportsPairExecution: true,
    recommendedRoles: ['mentor', 'executor']
  }
}

const legacy = model('minimax/MiniMax-M2.7', 'MiniMax M2.7')
const m3 = model('minimax/MiniMax-M3', 'MiniMax M3')
const flash = model('opencode/minimax-m3.1-flash-free', 'MiniMax M3.1 Flash')

afterEach(() => {
  cleanup()
  localStorage.clear()
})

test('picker lists Flash before M3 and resolves an active legacy model without resetting it', () => {
  const changes: string[] = []
  render(
    <ModelPicker
      value={legacy.modelId}
      models={[legacy, m3, flash]}
      onChange={(id) => changes.push(id)}
      role="mentor"
    />
  )
  const trigger = screen.getByRole('button')
  assert.match(trigger.textContent ?? '', /MiniMax M2.7/)
  fireEvent.click(trigger)
  const flashButton = screen.getByTitle('MiniMax M3.1 Flash')
  const m3Button = screen.getByTitle('MiniMax M3')
  const buttons = screen.getAllByRole('option')
  assert.ok(buttons.indexOf(flashButton) < buttons.indexOf(m3Button))
  assert.equal(screen.getAllByText('MiniMax M2.7').length, 1)
  assert.deepEqual(changes, [])
  fireEvent.click(flashButton)
  assert.deepEqual(changes, [flash.modelId])
  assert.equal(localStorage.getItem('the-pair-preferred-mentor-model'), flash.modelId)
})

test('explicit legacy recent history remains selectable', () => {
  localStorage.setItem('the-pair-recent-models-executor', JSON.stringify([legacy.modelId]))
  const changes: string[] = []
  render(
    <ModelPicker
      value={flash.modelId}
      models={[legacy, m3, flash]}
      onChange={(id) => changes.push(id)}
      role="executor"
    />
  )
  fireEvent.click(screen.getByRole('button', { name: /MiniMax M2.7/ }))
  assert.deepEqual(changes, [legacy.modelId])
})
