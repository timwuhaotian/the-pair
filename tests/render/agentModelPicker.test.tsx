import './setup-dom.ts'
import i18n from '../../src/renderer/src/i18n.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React, { useState } from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { assertAbsent } from './assert-dom.ts'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { ModelPicker } from '../../src/renderer/src/components/ModelPicker.tsx'
import type { AccountStatus, ModelRoute, ProviderView } from '../../src/renderer/src/types.ts'

function model(
  modelId: string,
  displayName: string,
  source: ModelRoute['source'] = 'preset'
): ModelRoute {
  return { id: modelId, displayName, efforts: [], source }
}

function provider(
  id: string,
  displayName: string,
  vendor: string,
  models: ModelRoute[],
  status: AccountStatus = 'ready'
): ProviderView {
  const accounts =
    status === 'ready'
      ? [
          {
            id: `${id}-key`,
            kind: 'apiKey' as const,
            label: 'Personal',
            source: 'manual',
            status: 'ready' as const,
            headers: [],
            hasSecret: true
          }
        ]
      : [
          {
            id: `${id}-key`,
            kind: 'apiKey' as const,
            label: 'Personal',
            source: 'manual',
            status,
            headers: [],
            hasSecret: false
          }
        ]
  return {
    id,
    protocol: 'openaiChat',
    baseUrl: `https://${vendor}.example`,
    vendor,
    displayName,
    accounts,
    models,
    readyAccountCount: status === 'ready' ? accounts.length : 0
  }
}

const claude = provider('claude-main', 'Claude Code', 'anthropic', [
  model('sonnet', 'Claude Sonnet'),
  model('opus', 'Claude Opus')
])
const codex = provider('codex-main', 'Codex', 'openai', [
  model('gpt-5', 'GPT-5'),
  model('gpt-5-mini', 'GPT-5 Mini')
])
const providers = [claude, codex]

function Harness({
  initial,
  onChange,
  registry = providers
}: {
  initial: string
  onChange: (id: string) => void
  registry?: ProviderView[]
}) {
  const [value, setValue] = useState(initial)
  return (
    <ModelPicker
      value={value}
      providers={registry}
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

test('an empty registry guides the user to Models and emits the navigation event', () => {
  const guide = 'Import a model configuration in Models'
  i18n.addResource(i18n.language, 'translation', 'pickers.noEndpointsGuide', guide)
  const calls: Array<{ cmd: string; args: unknown }> = []
  mockIPC((cmd, args) => {
    calls.push({ cmd, args })
  })
  try {
    const changes: string[] = []
    render(<Harness initial="" onChange={(id) => changes.push(id)} registry={[]} />)
    assert.ok(screen.getByText(guide))
    fireEvent.click(screen.getByRole('button', { name: i18n.t('pickers.openModels') }))
    assert.ok(
      calls.some(
        ({ cmd, args }) =>
          cmd === 'plugin:event|emit' && (args as { event?: string }).event === 'app:menu:go-models'
      )
    )
    assert.deepEqual(changes, [])
  } finally {
    clearMocks()
  }
})

test('a populated registry does not show the empty-registry guide', () => {
  const guide = 'Import a model configuration in Models'
  i18n.addResource(i18n.language, 'translation', 'pickers.noEndpointsGuide', guide)
  render(<Harness initial="sonnet" onChange={() => {}} />)
  assertAbsent(screen.queryByText(guide))
  assertAbsent(screen.queryByRole('button', { name: i18n.t('pickers.openModels') }))
})

test('step one follows the selection; step two lists only that endpoint, with vendor marks', () => {
  render(<Harness initial="opus" onChange={() => {}} />)
  const endpoints = screen.getAllByRole('radio')
  assert.deepEqual(
    endpoints.map((endpoint) => endpoint.getAttribute('aria-label')),
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

test('switching endpoint applies its first choice and opens its model list', () => {
  const changes: string[] = []
  render(<Harness initial="sonnet" onChange={(id) => changes.push(id)} />)
  fireEvent.click(screen.getByRole('radio', { name: 'Codex' }))
  assert.deepEqual(changes, ['gpt-5'])
  assert.equal(screen.getByRole('radio', { name: 'Codex' }).getAttribute('aria-checked'), 'true')
  const names = within(screen.getByRole('listbox'))
    .getAllByRole('option')
    .map((option) => option.title)
  assert.deepEqual(names.sort(), ['GPT-5', 'GPT-5 Mini'])
  assert.equal(localStorage.getItem('the-pair-preferred-mentor-model'), 'gpt-5')
})

test('a pick tells the backend which endpoint it was made under', () => {
  // Two endpoints list the same model id: the id alone cannot say which one
  // the turn should talk to, so the picked endpoint is reported with it.
  const plan = provider('chatgpt-plan', 'ChatGPT plan', 'openai', [model('gpt-5', 'GPT-5')])
  const key = provider('openai-key', 'OpenAI API', 'openai', [model('gpt-5', 'GPT-5')])
  const reported: Array<{ role: string; modelId: string; providerId: string }> = []
  const previousApi = window.api
  window.api = {
    models: {
      preferRoute: (input: { role: string; modelId: string; providerId: string }) => {
        reported.push(input)
        return Promise.resolve()
      }
    }
  } as unknown as typeof window.api
  try {
    const changes: string[] = []
    render(
      <Harness
        initial="sonnet"
        onChange={(id) => changes.push(id)}
        registry={[claude, plan, key]}
      />
    )
    fireEvent.click(screen.getByRole('radio', { name: 'OpenAI API' }))
    assert.deepEqual(changes, ['gpt-5'])
    assert.deepEqual(reported, [{ role: 'mentor', modelId: 'gpt-5', providerId: 'openai-key' }])

    fireEvent.click(screen.getByRole('radio', { name: 'ChatGPT plan' }))
    assert.deepEqual(reported.at(-1), {
      role: 'mentor',
      modelId: 'gpt-5',
      providerId: 'chatgpt-plan'
    })
  } finally {
    window.api = previousApi
  }
})

test('switching endpoint restores the most recent model used with it for this role', () => {
  localStorage.setItem('the-pair-recent-models-mentor', JSON.stringify(['sonnet', 'gpt-5-mini']))
  const changes: string[] = []
  render(<Harness initial="sonnet" onChange={(id) => changes.push(id)} />)
  fireEvent.click(screen.getByRole('radio', { name: 'Codex' }))
  assert.deepEqual(changes, ['gpt-5-mini'])
})

test('an endpoint with no ready account is listed with its credential state', () => {
  const expired = provider(
    'gemini-main',
    'Antigravity',
    'google',
    [model('gemini-3-pro', 'Gemini 3 Pro')],
    'authExpired'
  )
  const changes: string[] = []
  render(
    <Harness
      initial="sonnet"
      onChange={(id) => changes.push(id)}
      registry={[...providers, expired]}
    />
  )
  // Only the signed-in endpoints are selectable.
  assert.deepEqual(
    screen.getAllByRole('radio').map((radio) => radio.getAttribute('aria-label')),
    ['Claude Code', 'Codex']
  )
  fireEvent.click(screen.getByRole('button', { name: /Claude Sonnet/ }))
  const blocked = screen.getByRole('button', { name: /Antigravity — Sign-in expired/ })
  assert.match(blocked.textContent ?? '', /Sign-in expired/)
  fireEvent.click(blocked)
  // The row is a way into the Models section, never a model selection.
  assert.deepEqual(changes, [])
})
