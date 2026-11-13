import './setup-dom.ts'
import './mock-ipc.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test from 'node:test'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { mockIPC } from '@tauri-apps/api/mocks'
import { I18nextProvider } from 'react-i18next'
import { createServer } from 'vite'
import { assertAbsent } from './assert-dom.ts'

test('empty model picker opens Models and closes the create Pair dialog', async () => {
  const server = await createServer({ server: { middlewareMode: true }, appType: 'custom' })
  mockIPC(
    (cmd) => {
      if (
        cmd === 'accounts_list' ||
        cmd === 'load_all_pairs' ||
        cmd === 'list_recoverable_sessions'
      )
        return []
      if (cmd === 'accounts_login_services') return []
      if (cmd === 'get_insights_summary') return { runs: 0, interventions: 0, combos: [] }
      return undefined
    },
    { shouldMockEvents: true }
  )
  const originalAudio = globalThis.Audio
  globalThis.Audio = window.Audio
  try {
    const { default: App } = await server.ssrLoadModule('/src/App.tsx')
    const { default: i18n } = await server.ssrLoadModule('/src/i18n.ts')
    await i18n.changeLanguage('en')
    render(
      <I18nextProvider i18n={i18n}>
        <App />
      </I18nextProvider>
    )
    fireEvent.click(await screen.findByTestId('hub-empty-create', {}, { timeout: 5000 }))
    const dialog = await screen.findByRole('dialog')
    const guideButton = await within(dialog).findAllByRole('button', {
      name: i18n.t('pickers.openModels')
    })
    fireEvent.click(guideButton[0])
    await waitFor(() => {
      assert.ok(screen.getByTestId('models-view'))
      assertAbsent(screen.queryByRole('dialog'))
    })
  } finally {
    cleanup()
    globalThis.Audio = originalAudio
    await server.close()
  }
})
