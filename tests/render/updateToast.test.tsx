/**
 * Update toast: it must clear the app chrome bar (the chrome icons live in
 * the top-right corner) and announce itself politely, and it clears itself
 * after four seconds.
 */
import './setup-dom.ts'

// The tsx test loader compiles JSX with the classic runtime, so React must be in scope.
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { act, cleanup, render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import i18n from '../../src/renderer/src/i18n.ts'
import { UpdateNotification } from '../../src/renderer/src/components/UpdateNotification.tsx'
import { UpdateControls } from '../../src/renderer/src/components/UpdateControls.tsx'
import { useUpdateStore } from '../../src/renderer/src/store/useUpdateStore.ts'
import { assertAbsent } from './assert-dom.ts'

void i18n.changeLanguage('en')

afterEach(() => {
  cleanup()
  useUpdateStore.getState().reset()
})

function renderUpdate(): void {
  render(
    <I18nextProvider i18n={i18n}>
      <UpdateNotification />
    </I18nextProvider>
  )
}

function renderControls(): void {
  render(
    <I18nextProvider i18n={i18n}>
      <UpdateControls />
    </I18nextProvider>
  )
}

test('the update toast sits below the chrome bar and announces itself', () => {
  useUpdateStore.setState({ showToast: true, toastMessage: 'Boom', toastType: 'error' })
  renderUpdate()

  const toast = screen.getByRole('status')
  assert.equal(toast.getAttribute('aria-live'), 'polite')
  const classes = toast.className.split(/\s+/)
  assert.ok(classes.includes('top-12'), toast.className)
  assert.ok(!classes.includes('top-3'), toast.className)
  assert.ok(screen.getByText('Boom'))
})

test('the update toast clears itself after four seconds', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  useUpdateStore.setState({ showToast: true, toastMessage: 'Boom', toastType: 'error' })
  renderUpdate()
  assert.ok(screen.getByText('Boom'))

  act(() => {
    t.mock.timers.tick(4000)
  })
  assertAbsent(screen.queryByText('Boom'))
})

test('a failed check leaves a retryable button with a tooltip', () => {
  useUpdateStore.setState({ phase: 'error', message: null })
  renderControls()

  const button = screen.getByTestId('update-check-btn')
  assert.match(button.textContent ?? '', /Check again/)
  assert.equal(button.getAttribute('title'), "Couldn't check for updates")
})

test('a failed check keeps the backend reason in the tooltip', () => {
  useUpdateStore.setState({ phase: 'error', message: 'signature verification failed' })
  renderControls()

  const button = screen.getByTestId('update-check-btn')
  assert.match(button.textContent ?? '', /Check again/)
  assert.equal(button.getAttribute('title'), 'signature verification failed')
})

test('the update modal explains the failure and keeps the backend reason', () => {
  useUpdateStore.setState({
    phase: 'error',
    showModal: true,
    message: 'signature verification failed'
  })
  renderUpdate()

  // What happened, in the user's language...
  assert.ok(screen.getByText("Couldn't check for updates"))
  // ...and why, verbatim.
  assert.ok(screen.getByText('signature verification failed'))
  // The failure is announced assertively, unlike the polite toast.
  assert.ok(screen.getByRole('alert'))
})
