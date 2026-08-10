import assert from 'node:assert/strict'
import test from 'node:test'

import { useUpdateStore } from '../src/renderer/src/store/useUpdateStore.ts'

test.afterEach(() => {
  useUpdateStore.getState().reset()
})

test('displayToast sets toast state', () => {
  useUpdateStore.getState().displayToast('msg', 'success')
  const state = useUpdateStore.getState()
  assert.equal(state.showToast, true)
  assert.equal(state.toastMessage, 'msg')
  assert.equal(state.toastType, 'success')
})

test('clearToast resets toast state', () => {
  useUpdateStore.getState().displayToast('hello', 'error')
  useUpdateStore.getState().clearToast()
  const state = useUpdateStore.getState()
  assert.equal(state.showToast, false)
  assert.equal(state.toastMessage, null)
  assert.equal(state.toastType, null)
})

test('setPhase sets phase', () => {
  useUpdateStore.getState().setPhase('available')
  assert.equal(useUpdateStore.getState().phase, 'available')
})

test('reset returns to initial state', () => {
  useUpdateStore.getState().setPhase('available')
  useUpdateStore.getState().setVersion('1.2.3')
  useUpdateStore.getState().displayToast('msg', 'info')
  useUpdateStore.getState().reset()
  const state = useUpdateStore.getState()
  assert.equal(state.phase, 'idle')
  assert.equal(state.version, null)
  assert.equal(state.showToast, false)
})

test('installUpdate returns immediately without update set', async () => {
  // Guard clause: no update → should not throw
  await useUpdateStore.getState().installUpdate()
  // State should remain unchanged (not 'installing')
  assert.notEqual(useUpdateStore.getState().phase, 'installing')
})
