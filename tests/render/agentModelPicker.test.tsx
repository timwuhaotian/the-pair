import './setup-dom.ts'
import i18n from '../../src/renderer/src/i18n.ts'
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

test('the model list is a combobox: arrows, Home and End move the highlight and Enter picks it', () => {
  const changes: string[] = []
  render(<Harness initial="gpt-5" onChange={(id) => changes.push(id)} />)
  const trigger = screen.getByRole('button', { name: /GPT-5/ })
  fireEvent.click(trigger)
  const input = screen.getByRole('combobox', { name: i18n.t('pickers.searchModels') })
  const listbox = screen.getByRole('listbox')
  assert.equal(input.getAttribute('aria-controls'), listbox.id)
  const options = within(listbox).getAllByRole('option')
  const idOf = (title: string): string | undefined =>
    options.find((option) => option.title === title)?.id
  // The list opens on the model in use.
  assert.equal(input.getAttribute('aria-activedescendant'), idOf('GPT-5'))
  assert.ok(options.every((option) => option.getAttribute('tabindex') === '-1'))

  fireEvent.keyDown(input, { key: 'End' })
  const last = options[options.length - 1]
  assert.equal(input.getAttribute('aria-activedescendant'), last.id)
  assert.equal(last.getAttribute('aria-selected'), 'true')
  fireEvent.keyDown(input, { key: 'Home' })
  assert.equal(input.getAttribute('aria-activedescendant'), options[0].id)
  fireEvent.keyDown(input, { key: 'ArrowDown' })
  assert.equal(input.getAttribute('aria-activedescendant'), options[1].id)

  // Enter picks the highlighted row even with several matches, and focus
  // goes back to the trigger instead of <body>.
  input.focus()
  fireEvent.keyDown(input, { key: 'Enter' })
  assert.equal(options[1].title, 'GPT-5 Mini')
  assert.deepEqual(changes, ['gpt-5-mini'])
  assert.ok(!screen.queryByRole('listbox'), 'list closed')
  assert.ok(document.activeElement === trigger, 'focus is back on the trigger')
})

test('Escape in the model list closes it and refocuses the trigger', () => {
  render(<Harness initial="gpt-5" onChange={() => undefined} />)
  const trigger = screen.getByRole('button', { name: /GPT-5/ })
  fireEvent.click(trigger)
  const input = screen.getByRole('combobox', { name: i18n.t('pickers.searchModels') })
  input.focus()
  fireEvent.keyDown(input, { key: 'Escape' })
  assert.ok(!screen.queryByRole('listbox'), 'list closed')
  assert.ok(document.activeElement === trigger, 'focus is back on the trigger')
})

test('the model trigger is labelled by the Model field label', () => {
  render(<Harness initial="opus" onChange={() => undefined} />)
  const trigger = screen.getByRole('button', { name: /Claude Opus/ })
  const ids = (trigger.getAttribute('aria-labelledby') ?? '').split(' ')
  assert.ok(document.getElementById(ids[0])?.textContent === i18n.t('pickers.model'))
  assert.ok(ids[1] === trigger.id, 'the trigger names itself after the label')
})

test('endpoint radios use a roving tab stop; arrows move focus without switching', () => {
  const changes: string[] = []
  render(<Harness initial="sonnet" onChange={(id) => changes.push(id)} />)
  const claudeRadio = screen.getByRole('radio', { name: 'Claude Code' })
  const codexRadio = screen.getByRole('radio', { name: 'Codex' })
  assert.equal(claudeRadio.getAttribute('tabindex'), '0')
  assert.equal(codexRadio.getAttribute('tabindex'), '-1')

  claudeRadio.focus()
  fireEvent.keyDown(claudeRadio, { key: 'ArrowDown' })
  assert.ok(document.activeElement === codexRadio, 'ArrowDown moved focus to the next endpoint')
  // Switching endpoint has side effects (applies a model, opens the list), so
  // arrows only move focus; Space/Enter picks.
  assert.deepEqual(changes, [])
  fireEvent.keyDown(codexRadio, { key: 'ArrowDown' })
  assert.ok(document.activeElement === claudeRadio, 'ArrowDown wraps')
})

test('the card variant is a group named after its role; reasoning radios select on arrows', () => {
  const thinking = provider('claude-think', 'Claude Code', 'anthropic', [
    {
      id: 'sonnet',
      displayName: 'Claude Sonnet',
      efforts: ['low', 'medium', 'high'],
      source: 'preset'
    }
  ])
  const efforts: Array<string | undefined> = []
  function CardHarness(): React.ReactNode {
    const [effort, setEffort] = useState<string | undefined>('low')
    return (
      <ModelPicker
        value="sonnet"
        providers={[thinking]}
        role="executor"
        variant="card"
        reasoningEffort={effort}
        onChange={() => undefined}
        onReasoningEffortChange={(value) => {
          efforts.push(value)
          setEffort(value)
        }}
      />
    )
  }
  render(<CardHarness />)
  const card = screen.getByRole('group', { name: i18n.t('pickers.executorLabel') })
  assert.ok(within(card).getByRole('radiogroup', { name: i18n.t('pickers.reasoning') }))

  const reasoning = within(card).getByRole('radiogroup', { name: i18n.t('pickers.reasoning') })
  const radios = within(reasoning).getAllByRole('radio')
  const checked = radios.find((radio) => radio.getAttribute('aria-checked') === 'true')
  assert.ok(checked, 'one effort is checked')
  assert.equal(
    radios.filter((radio) => radio.getAttribute('tabindex') === '0').length,
    1,
    'one Tab stop'
  )
  assert.ok(checked.getAttribute('tabindex') === '0', 'the checked radio is the Tab stop')

  checked.focus()
  fireEvent.keyDown(checked, { key: 'ArrowRight' })
  assert.deepEqual(efforts, ['medium'])
  const nowChecked = within(reasoning)
    .getAllByRole('radio')
    .find((radio) => radio.getAttribute('aria-checked') === 'true')
  assert.ok(nowChecked && document.activeElement === nowChecked, 'focus follows the selection')
})
