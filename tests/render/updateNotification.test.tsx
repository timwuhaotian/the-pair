/**
 * Component tests for the update modal: each phase a manual check walks
 * through shows its own feedback, and an installed update asks before it
 * restarts (Later keeps it installed for the next launch).
 */
import './setup-dom.ts'

import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import i18n from '../../src/renderer/src/i18n.ts'
import { UpdateNotification } from '../../src/renderer/src/components/UpdateNotification.tsx'
import { useUpdateStore } from '../../src/renderer/src/store/useUpdateStore.ts'

void i18n.changeLanguage('en')

afterEach(() => {
  cleanup()
  useUpdateStore.getState().reset()
})

function Modal(): React.ReactNode {
  return (
    <I18nextProvider i18n={i18n}>
      <UpdateNotification />
    </I18nextProvider>
  )
}

function renderModal(): void {
  render(<Modal />)
}

test('a manual check shows checking, then up to date', () => {
  useUpdateStore.setState({ phase: 'checking', showModal: true })
  renderModal()
  assert.ok(screen.getByText('Checking for updates'))
  assert.ok(screen.getByTestId('update-progress'))

  fireEvent.click(screen.getByTestId('update-close'))
  // Closing mid-check only hides the modal; the check still lands.
  assert.equal(useUpdateStore.getState().phase, 'checking')
  assert.equal(useUpdateStore.getState().showModal, false)
})

test('up to date says so', () => {
  useUpdateStore.setState({ phase: 'up-to-date', showModal: true })
  renderModal()
  assert.ok(screen.getByText("You're up to date"))
})

test('downloading shows percent, megabytes and the active step', () => {
  useUpdateStore.setState({
    phase: 'installing',
    showModal: true,
    version: '9.9.9',
    progress: 45,
    downloadedBytes: 12 * 1024 * 1024,
    totalBytes: 27 * 1024 * 1024
  })
  renderModal()
  assert.ok(screen.getByText('Updating to v9.9.9'))
  assert.ok(screen.getByText('Downloading 45%'))
  assert.ok(screen.getByText('12.0 MB / 27.0 MB'))
  const bar = screen.getByTestId('update-progress').querySelector('[role="progressbar"]')
  assert.equal(bar?.getAttribute('aria-valuenow'), '45')
  // No buttons mid-install: it can't be cancelled or dismissed.
  assert.equal(screen.queryByText('Later'), null)
  assert.equal(screen.queryByText('Close'), null)
})

test('a finished download shows installing', () => {
  useUpdateStore.setState({ phase: 'installing', showModal: true, version: '9.9.9', progress: 100 })
  renderModal()
  assert.ok(screen.getByText('Download complete. Installing the update…'))
})

test('ready asks to restart; Later keeps the installed update', () => {
  useUpdateStore.setState({ phase: 'ready', showModal: true, version: '9.9.9', progress: 100 })
  renderModal()
  assert.ok(screen.getByText('v9.9.9 is ready'))
  assert.ok(screen.getByTestId('update-restart-now'))

  fireEvent.click(screen.getByTestId('update-restart-later'))
  assert.equal(useUpdateStore.getState().phase, 'ready')
  assert.equal(useUpdateStore.getState().showModal, false)
})

test('Restart now restarts the app', async () => {
  let restarts = 0
  const original = window.api
  ;(window as unknown as { api: unknown }).api = {
    ...original,
    app: { ...original?.app, restart: async () => void (restarts += 1) }
  }
  try {
    useUpdateStore.setState({ phase: 'ready', showModal: true, version: '9.9.9', progress: 100 })
    renderModal()
    fireEvent.click(screen.getByTestId('update-restart-now'))
    await new Promise((resolve) => setTimeout(resolve, 0))
    assert.equal(restarts, 1)
  } finally {
    ;(window as unknown as { api: unknown }).api = original
  }
})
