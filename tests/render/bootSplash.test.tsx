/**
 * Component tests for the boot splash: it covers the window until the app is
 * ready, fades, and then leaves the DOM.
 */
import './setup-dom.ts'
import './mock-ipc.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { I18nextProvider } from 'react-i18next'
import { cleanup, render, waitFor } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import { BootSplash } from '../../src/renderer/src/components/BootSplash.tsx'
import { assertAbsent } from './assert-dom.ts'

afterEach(cleanup)

function renderSplash(visible: boolean): ReturnType<typeof render> {
  return render(
    <I18nextProvider i18n={i18n}>
      <BootSplash visible={visible} minDurationMs={0} />
    </I18nextProvider>
  )
}

test('the splash shows the animated brand mark while the app loads', () => {
  const { container } = renderSplash(true)
  const splash = container.querySelector('.boot-splash')
  assert.ok(splash, 'the splash is missing')
  assert.ok(splash.querySelector('.brand-mark--animated'), 'the mark is not animated')
  assert.doesNotMatch(splash.className, /boot-splash--leaving/)
})

test('once the app is ready the splash fades and then unmounts', async () => {
  const { container } = renderSplash(false)

  await waitFor(() => {
    assert.match(container.querySelector('.boot-splash')?.className ?? '', /boot-splash--leaving/)
  })
  // The fade takes 460 ms; after it the overlay — and the mark's endless idle
  // animation with it — has to be gone, not merely transparent.
  await waitFor(() => assertAbsent(container.querySelector('.boot-splash'), 'the splash stayed'), {
    timeout: 2000
  })
})
