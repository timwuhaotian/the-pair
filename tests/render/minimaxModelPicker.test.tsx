import './setup-dom.ts'
import '../../src/renderer/src/i18n.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { ModelPicker } from '../../src/renderer/src/components/ModelPicker.tsx'
import type { ModelRoute, ProviderView } from '../../src/renderer/src/types.ts'

function model(modelId: string, displayName: string): ModelRoute {
  return { id: modelId, displayName, efforts: [], source: 'imported' }
}

/** One MiniMax endpoint carrying three generations of the same text model. */
function minimaxProvider(models: ModelRoute[]): ProviderView {
  return {
    id: 'minimax-main',
    protocol: 'anthropic',
    baseUrl: 'https://api.minimax.example',
    vendor: 'minimax',
    displayName: 'MiniMax',
    accounts: [
      {
        id: 'minimax-key',
        kind: 'apiKey',
        label: 'Personal',
        source: 'manual',
        status: 'ready',
        headers: [],
        hasSecret: true
      }
    ],
    models,
    readyAccountCount: 1
  }
}

const legacy = model('minimax-m2.7', 'MiniMax M2.7')
const m3 = model('minimax-m3', 'MiniMax M3')
const flash = model('minimax-m3.1-flash-free', 'MiniMax M3.1 Flash')

afterEach(() => {
  cleanup()
  localStorage.clear()
})

test('picker lists Flash before M3 and resolves an active legacy model without resetting it', () => {
  const changes: string[] = []
  render(
    <ModelPicker
      value={legacy.id}
      providers={[minimaxProvider([legacy, m3, flash])]}
      onChange={(id) => changes.push(id)}
      role="mentor"
    />
  )
  const trigger = screen.getByRole('button', { name: /MiniMax M2.7/ })
  assert.match(trigger.textContent ?? '', /MiniMax M2.7/)
  fireEvent.click(trigger)
  const flashButton = screen.getByTitle('MiniMax M3.1 Flash')
  const m3Button = screen.getByTitle('MiniMax M3')
  const buttons = screen.getAllByRole('option')
  assert.ok(buttons.indexOf(flashButton) < buttons.indexOf(m3Button))
  assert.equal(screen.getAllByText('MiniMax M2.7').length, 1)
  assert.deepEqual(changes, [])
  fireEvent.click(flashButton)
  assert.deepEqual(changes, [flash.id])
  assert.equal(localStorage.getItem('the-pair-preferred-mentor-model'), flash.id)
})

test('explicit legacy recent history remains selectable', () => {
  localStorage.setItem('the-pair-recent-models-executor', JSON.stringify([legacy.id]))
  const changes: string[] = []
  render(
    <ModelPicker
      value={flash.id}
      providers={[minimaxProvider([legacy, m3, flash])]}
      onChange={(id) => changes.push(id)}
      role="executor"
    />
  )
  // Recent rows are radios in a radiogroup (selection state exposed to AT).
  fireEvent.click(screen.getByRole('radio', { name: /MiniMax M2.7/ }))
  assert.deepEqual(changes, [legacy.id])
})
