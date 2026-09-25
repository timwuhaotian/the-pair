import assert from 'node:assert/strict'
import test from 'node:test'

import { detectTauriRuntime, isTauri, tauriApi } from '../src/renderer/src/lib/tauri-api.ts'
import { mockRepoState } from '../src/renderer/src/lib/mock-data.ts'

test('tauriApi can be imported outside the Tauri renderer', async () => {
  assert.equal(isTauri, false)
  assert.deepEqual(await tauriApi.repo.checkState('/tmp/project'), mockRepoState)
  await assert.rejects(tauriApi.pair.list(), /Not running in Tauri/)
})

test('detectTauriRuntime recognizes a Tauri 2 webview without withGlobalTauri', () => {
  // The packaged app never defines window.__TAURI__ (withGlobalTauri is off), but
  // every Tauri 2 webview gets __TAURI_INTERNALS__ and globalThis.isTauri.
  assert.equal(detectTauriRuntime({ __TAURI_INTERNALS__: {} }), true)
  assert.equal(detectTauriRuntime({ isTauri: true }), true)
})

test('detectTauriRuntime is false in a plain browser or Node', () => {
  assert.equal(detectTauriRuntime({}), false)
  assert.equal(detectTauriRuntime({ isTauri: 'yes' }), false)
  assert.equal(detectTauriRuntime(undefined), false)
  assert.equal(detectTauriRuntime(), false)
})
