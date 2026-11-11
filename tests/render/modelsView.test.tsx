/**
 * Component tests for the models section: the provider list, the import
 * wizard's "what will be copied" contract, and the sign-in modal's two flows.
 * The wizard must never show a raw key — only the masked preview.
 */
import './setup-dom.ts'
import './mock-ipc.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach, beforeEach } from 'node:test'
import { I18nextProvider } from 'react-i18next'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import { ModelsView } from '../../src/renderer/src/components/ModelsView.tsx'
import { registry } from './mock-ipc.ts'

function renderView(): void {
  render(
    <I18nextProvider i18n={i18n}>
      <ModelsView />
    </I18nextProvider>
  )
}

beforeEach(() => {
  registry.calls.length = 0
})

afterEach(() => {
  cleanup()
})

test('lists every provider with its endpoint, accounts and models', async () => {
  renderView()
  await waitFor(() => assert.ok(screen.getByTestId('models-view')))

  const anthropic = screen.getByTestId('provider-card-anthropic-1')
  assert.match(anthropic.textContent ?? '', /Anthropic/)
  assert.match(anthropic.textContent ?? '', /https:\/\/api\.anthropic\.com/)
  assert.match(anthropic.textContent ?? '', /1\/1 accounts/)
  assert.match(anthropic.textContent ?? '', /1 models/)

  const openai = screen.getByTestId('provider-card-openai-1')
  assert.match(openai.textContent ?? '', /No credential/)
  assert.match(openai.textContent ?? '', /0\/0 accounts/)
})

test('expanding a provider shows accounts without leaking the key', async () => {
  renderView()
  await waitFor(() => assert.ok(screen.getByTestId('provider-card-anthropic-1')))
  fireEvent.click(screen.getByTestId('provider-card-anthropic-1').querySelector('button')!)

  const card = screen.getByTestId('provider-card-anthropic-1')
  assert.match(card.textContent ?? '', /ANTHROPIC_API_KEY/)
  assert.match(card.textContent ?? '', /sk-ant…mnop/)
  assert.match(card.textContent ?? '', /Claude Sonnet 4\.5/)
  assert.ok(!/sk-ant-api03|super-secret/.test(card.textContent ?? ''))
})

test('the import wizard lists candidates and commits the checked ones', async () => {
  renderView()
  await waitFor(() => assert.ok(screen.getByTestId('models-scan')))
  fireEvent.click(screen.getByTestId('models-scan'))

  const row = await waitFor(() => screen.getByTestId('import-row-kimi'))
  assert.match(row.textContent ?? '', /Kimi Code/)
  assert.match(row.textContent ?? '', /https:\/\/api\.moonshot\.cn\/v1/)
  assert.match(row.textContent ?? '', /sk-ki…abcd/)
  assert.ok(!/sk-kimi-real/.test(row.textContent ?? ''))

  fireEvent.click(screen.getByTestId('import-wizard-commit'))
  await waitFor(() => {
    const commit = registry.calls.find((call) => call.cmd === 'accounts_commit_import')
    assert.ok(commit, 'commit was invoked')
    assert.deepEqual(commit.args, { sourceIds: ['kimi'] })
  })
})

test('a source with nothing to import cannot be checked', async () => {
  registry.candidates = [
    {
      sourceId: 'claude',
      sourceName: 'Claude Code',
      accountCount: 0,
      newProviderCount: 0,
      notes: ['Claude Code 订阅无法导入'],
      providers: []
    }
  ]
  try {
    renderView()
    await waitFor(() => assert.ok(screen.getByTestId('models-scan')))
    fireEvent.click(screen.getByTestId('models-scan'))

    const row = await waitFor(() => screen.getByTestId('import-row-claude'))
    const checkbox = row.querySelector('input[type="checkbox"]') as HTMLInputElement
    assert.ok(checkbox.disabled, 'an empty source must not be importable')
    assert.match(row.textContent ?? '', /订阅无法导入/)
    assert.ok(screen.getByTestId('import-wizard-commit').hasAttribute('disabled'))
  } finally {
    registry.candidates = [
      {
        sourceId: 'kimi',
        sourceName: 'Kimi Code',
        configPath: '/home/dev/.kimi-code/config.toml',
        accountCount: 1,
        newProviderCount: 1,
        notes: [],
        providers: [
          {
            protocol: 'openaiChat',
            baseUrl: 'https://api.moonshot.cn/v1',
            vendor: 'moonshot',
            displayName: 'Moonshot (Kimi Code)',
            alreadyImported: false,
            models: [],
            accounts: [
              {
                kind: 'apiKey',
                label: 'moonshot',
                source: 'kimi',
                status: 'ready',
                maskedSecret: 'sk-ki…abcd',
                headers: []
              }
            ]
          }
        ]
      }
    ]
  }
})

test('the sign-in modal labels browser and key flows differently', async () => {
  renderView()
  await waitFor(() => assert.ok(screen.getByTestId('models-sign-in')))
  fireEvent.click(screen.getByTestId('models-sign-in'))

  await waitFor(() => assert.ok(screen.getByTestId('sign-in-chatgpt')))
  const modal = screen.getByTestId('sign-in-modal')
  assert.match(modal.textContent ?? '', /Opens your browser/)
  assert.match(modal.textContent ?? '', /Paste an API key/)
  assert.match(modal.textContent ?? '', /ChatGPT/)

  // Every key service offers the key field up front, so a key-only service
  // never has to fail once before the field appears.
  assert.ok(screen.queryByTestId('sign-in-key'), 'the key field is offered up front')
  fireEvent.click(screen.getByTestId('sign-in-muse'))
  await waitFor(() => assert.ok(screen.getByTestId('sign-in-key')))
})

test('a browser sign-in polls for status', async () => {
  renderView()
  await waitFor(() => assert.ok(screen.getByTestId('models-sign-in')))
  fireEvent.click(screen.getByTestId('models-sign-in'))
  await waitFor(() => assert.ok(screen.getByTestId('sign-in-chatgpt')))
  fireEvent.click(screen.getByTestId('sign-in-chatgpt'))

  await waitFor(() => {
    const launch = registry.calls.find((call) => call.cmd === 'accounts_launch_oauth')
    assert.ok(launch, 'launch was invoked')
    assert.deepEqual(launch.args, { service: 'chatgpt', apiKey: null })
  })
  await waitFor(() => {
    const waiting = screen.getAllByText('Waiting')
    assert.ok(waiting.length >= 2, 'the button and the status chip both show Waiting')
  })
})

test('adding an endpoint by hand requires a base URL', async () => {
  renderView()
  await waitFor(() => assert.ok(screen.getByTestId('models-add-provider')))
  fireEvent.click(screen.getByTestId('models-add-provider'))
  fireEvent.click(screen.getByTestId('add-provider-save'))

  await waitFor(() =>
    assert.match(screen.getByTestId('add-provider-modal').textContent ?? '', /base URL is required/)
  )
  assert.ok(!registry.calls.some((call) => call.cmd === 'accounts_add_provider'))

  fireEvent.change(screen.getByTestId('add-provider-base-url'), {
    target: { value: 'https://llm.internal.example/v1' }
  })
  fireEvent.change(screen.getByTestId('add-provider-models'), {
    target: { value: 'internal-1, internal-2' }
  })
  fireEvent.click(screen.getByTestId('add-provider-save'))

  await waitFor(() => {
    const add = registry.calls.find((call) => call.cmd === 'accounts_add_provider')
    assert.ok(add, 'add was invoked')
    const input = (add.args as { input: Record<string, unknown> }).input
    assert.equal(input.baseUrl, 'https://llm.internal.example/v1')
    assert.deepEqual(input.models, ['internal-1', 'internal-2'])
  })
})

test('an endpoint whose URL has no host is refused before the backend sees it', async () => {
  renderView()
  await waitFor(() => assert.ok(screen.getByTestId('models-add-provider')))
  fireEvent.click(screen.getByTestId('models-add-provider'))

  // A path-only or scheme-less URL normalises to nothing, which the backend
  // rejects; the form refuses it with its own message instead.
  for (const value of ['/', '///', 'api.example.com']) {
    fireEvent.change(screen.getByTestId('add-provider-base-url'), { target: { value } })
    fireEvent.click(screen.getByTestId('add-provider-save'))
    await waitFor(() =>
      assert.match(
        screen.getByTestId('add-provider-modal').textContent ?? '',
        /including http:\/\/ or https:\/\//
      )
    )
  }
  assert.ok(!registry.calls.some((call) => call.cmd === 'accounts_add_provider'))
})

test('the add-provider draft is empty the next time it opens', async () => {
  renderView()
  await waitFor(() => assert.ok(screen.getByTestId('models-add-provider')))
  fireEvent.click(screen.getByTestId('models-add-provider'))
  fireEvent.change(screen.getByTestId('add-provider-base-url'), {
    target: { value: 'https://stale.example/v1' }
  })
  fireEvent.change(screen.getByTestId('add-provider-models'), {
    target: { value: 'stale-1' }
  })
  fireEvent.click(screen.getByTestId('add-provider-cancel'))

  fireEvent.click(screen.getByTestId('models-add-provider'))
  const baseUrl = screen.getByTestId('add-provider-base-url') as HTMLInputElement
  const models = screen.getByTestId('add-provider-models') as HTMLInputElement
  assert.equal(baseUrl.value, '', 'a cancelled draft must not be offered again')
  assert.equal(models.value, '')
})

test('closing the sign-in modal clears the flow it was in', async () => {
  renderView()
  await waitFor(() => assert.ok(screen.getByTestId('models-sign-in')))
  fireEvent.click(screen.getByTestId('models-sign-in'))
  await waitFor(() => assert.ok(screen.getByTestId('sign-in-chatgpt')))

  // Start a browser flow, then walk away: the modal refuses to close while a
  // browser flow waits, so the walk-away is a cancel.
  fireEvent.click(screen.getByTestId('sign-in-chatgpt'))
  await waitFor(() =>
    assert.match(screen.getByTestId('sign-in-modal').textContent ?? '', /Waiting/)
  )
  assert.ok(screen.getByTestId('modal-close').hasAttribute('disabled'), 'cannot close mid-wait')
  fireEvent.click(screen.getByTestId('sign-in-cancel'))
  await waitFor(() =>
    assert.ok(screen.getByTestId('modal-close').hasAttribute('disabled') === false)
  )
  fireEvent.click(screen.getByTestId('modal-close'))

  fireEvent.click(screen.getByTestId('models-sign-in'))
  await waitFor(() => assert.ok(screen.getByTestId('sign-in-chatgpt')))
  assert.ok(
    !/Waiting/.test(screen.getByTestId('sign-in-modal').textContent ?? ''),
    'no flow is still waiting'
  )
  assert.ok(screen.queryByTestId('sign-in-chatgpt')?.hasAttribute('disabled') === false)
  assert.ok(screen.queryByTestId('sign-in-muse')?.hasAttribute('disabled') === false)
})

test('a delete the backend refuses is reported instead of swallowed', async () => {
  const realInvoke = window.__TAURI_INTERNALS__.invoke
  window.__TAURI_INTERNALS__.invoke = async (cmd: string, args?: unknown) => {
    if (cmd === 'accounts_delete_account') {
      throw new Error('the credential file is locked')
    }
    return realInvoke(cmd, args)
  }
  try {
    renderView()
    await waitFor(() => assert.ok(screen.getByTestId('provider-card-anthropic-1')))
    fireEvent.click(screen.getByTestId('provider-card-anthropic-1').querySelector('button')!)
    fireEvent.click(screen.getAllByTestId('account-delete')[0])

    await waitFor(() => assert.match(screen.getByTestId('models-view').textContent ?? '', /locked/))
  } finally {
    window.__TAURI_INTERNALS__.invoke = realInvoke
  }
})
