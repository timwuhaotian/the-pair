import assert from 'node:assert/strict'
import test from 'node:test'

import { isTauri, tauriApi } from '../src/renderer/src/lib/tauri-api.ts'
import { mockRepoState } from '../src/renderer/src/lib/mock-data.ts'

test('tauriApi can be imported outside the Tauri renderer', async () => {
  assert.equal(isTauri, false)
  assert.deepEqual(await tauriApi.repo.checkState('/tmp/project'), mockRepoState)
  await assert.rejects(tauriApi.pair.list(), /Not running in Tauri/)
})
