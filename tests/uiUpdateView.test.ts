import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

import {
  canDismissUpdateModal,
  canStartUpdateCheck,
  shouldShowUpdateModal
} from '../src/renderer/src/components/updateView.ts'

test('update modal renders for available, installing and error phases (C8)', () => {
  assert.equal(shouldShowUpdateModal(true, 'available'), true)
  assert.equal(shouldShowUpdateModal(true, 'installing'), true)
  assert.equal(shouldShowUpdateModal(true, 'error'), true)
  assert.equal(shouldShowUpdateModal(true, 'checking'), false)
  assert.equal(shouldShowUpdateModal(true, 'up-to-date'), false)
  assert.equal(shouldShowUpdateModal(false, 'installing'), false)
})

test('no new update check and no dismissal while installing', () => {
  assert.equal(canStartUpdateCheck('installing'), false)
  assert.equal(canStartUpdateCheck('available'), true)
  assert.equal(canStartUpdateCheck('error'), true)
  assert.equal(canDismissUpdateModal('installing'), false)
  assert.equal(canDismissUpdateModal('error'), true)
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
