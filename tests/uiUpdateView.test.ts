import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

import {
  canDismissUpdateModal,
  canStartUpdateCheck,
  formatMegabytes,
  keepsStateOnDismiss,
  shouldShowUpdateModal
} from '../src/renderer/src/components/updateView.ts'

test('an open update modal renders every phase but idle (C8)', () => {
  for (const phase of ['checking', 'up-to-date', 'available', 'installing', 'ready', 'error']) {
    assert.equal(shouldShowUpdateModal(true, phase), true, phase)
  }
  assert.equal(shouldShowUpdateModal(true, 'idle'), false)
  assert.equal(shouldShowUpdateModal(false, 'installing'), false)
})

test('no new update check mid-check, while installing, or with an installed update', () => {
  assert.equal(canStartUpdateCheck('checking'), false)
  assert.equal(canStartUpdateCheck('ready'), false)
  assert.equal(canStartUpdateCheck('up-to-date'), true)
  assert.equal(canStartUpdateCheck('installing'), false)
  assert.equal(canStartUpdateCheck('available'), true)
  assert.equal(canStartUpdateCheck('error'), true)
  assert.equal(canDismissUpdateModal('installing'), false)
  assert.equal(canDismissUpdateModal('error'), true)
})

test('dismissing keeps an in-flight check and an installed update, resets the rest', () => {
  assert.equal(keepsStateOnDismiss('checking'), true)
  assert.equal(keepsStateOnDismiss('ready'), true)
  assert.equal(keepsStateOnDismiss('available'), false)
  assert.equal(keepsStateOnDismiss('up-to-date'), false)
  assert.equal(keepsStateOnDismiss('error'), false)
})

test('download sizes read as megabytes with one decimal', () => {
  assert.equal(formatMegabytes(0), '0.0 MB')
  assert.equal(formatMegabytes(13_002_342), '12.4 MB')
})

test('a manual check opens the modal; the startup check stays silent', async () => {
  const app = await readFile(new URL('../src/renderer/src/App.tsx', import.meta.url), 'utf8')
  assert.match(app, /if \(manual\) setShowModal\(true\)/)
  assert.match(app, /listen\('app:update:check', \(\) => \{\s*void performUpdateCheck\(true\)/)
  assert.match(app, /queueStartupUpdateCheck\(\(\) => \{\s*void performUpdateCheck\(false\)/)
})

test('App guards the update check and the modal uses the shared visibility rule', async () => {
  const app = await readFile(new URL('../src/renderer/src/App.tsx', import.meta.url), 'utf8')
  const modal = await readFile(
    new URL('../src/renderer/src/components/UpdateNotification.tsx', import.meta.url),
    'utf8'
  )
  assert.match(app, /canStartUpdateCheck\(useUpdateStore\.getState\(\)\.phase\)/)
  assert.match(modal, /shouldShowUpdateModal\(showModal, phase\)/)
  assert.doesNotMatch(modal, /showModal && phase === 'available'/)
})
