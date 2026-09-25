import assert from 'node:assert/strict'
import test from 'node:test'

import { describeInstallError, useUpdateStore } from '../src/renderer/src/store/useUpdateStore.ts'

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

type DownloadEvent =
  | { event: 'Started'; data: { contentLength?: number } }
  | { event: 'Progress'; data: { chunkLength: number } }
  | { event: 'Finished' }

function fakeUpdate(
  run: (emit: (event: DownloadEvent) => void) => Promise<void>
): NonNullable<ReturnType<typeof useUpdateStore.getState>['update']> {
  return {
    downloadAndInstall: (onEvent: (event: DownloadEvent) => void) => run(onEvent),
    close: async () => {}
  } as unknown as NonNullable<ReturnType<typeof useUpdateStore.getState>['update']>
}

test('installUpdate keeps the modal open while installing', async () => {
  const seen: Array<{ phase: string; showModal: boolean }> = []
  useUpdateStore.setState({ phase: 'available', showModal: true })
  useUpdateStore.getState().setUpdate(
    fakeUpdate(async (emit) => {
      emit({ event: 'Started', data: { contentLength: 100 } })
      emit({ event: 'Progress', data: { chunkLength: 50 } })
      const { phase, showModal, progress } = useUpdateStore.getState()
      seen.push({ phase, showModal })
      assert.equal(progress, 50)
      throw new Error('stop here')
    })
  )

  await useUpdateStore.getState().installUpdate()
  assert.deepEqual(seen, [{ phase: 'installing', showModal: true }])
})

test('installUpdate failure shows a readable error in the modal', async () => {
  useUpdateStore.setState({ phase: 'available', showModal: true })
  useUpdateStore.getState().setUpdate(
    fakeUpdate(async () => {
      throw 'signature verification failed'
    })
  )

  await useUpdateStore.getState().installUpdate()
  const state = useUpdateStore.getState()
  assert.equal(state.phase, 'error')
  assert.equal(state.showModal, true)
  assert.equal(state.message, 'Update installation failed: signature verification failed')
})

test('installUpdate is a no-op while an install is already running', async () => {
  let calls = 0
  useUpdateStore.setState({ phase: 'installing', showModal: true })
  useUpdateStore.getState().setUpdate(
    fakeUpdate(async () => {
      calls += 1
    })
  )
  await useUpdateStore.getState().installUpdate()
  assert.equal(calls, 0)
})

test('describeInstallError never doubles the lead-in and handles empty errors', () => {
  assert.equal(describeInstallError(new Error('')), 'Update installation failed.')
  assert.equal(
    describeInstallError('Update installation failed: disk full'),
    'Update installation failed: disk full'
  )
  assert.equal(describeInstallError(new Error('timeout')), 'Update installation failed: timeout')
})

test('installUpdate returns immediately without update set', async () => {
  // Guard clause: no update → should not throw
  await useUpdateStore.getState().installUpdate()
  // State should remain unchanged (not 'installing')
  assert.notEqual(useUpdateStore.getState().phase, 'installing')
})
