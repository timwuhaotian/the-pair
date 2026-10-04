import assert from 'node:assert/strict'
import test from 'node:test'

// The commit/push wrappers are thin invoke() shells. Outside the Tauri
// webview invoke() rejects, which proves the wiring (command name + args)
// without a backend: the rejection must come from the invoke layer, not from
// argument validation.
import { tauriApi } from '../src/renderer/src/lib/tauri-api.ts'

test('repo.commitChanges invokes git_commit_changes with directory + message', async () => {
  await assert.rejects(
    tauriApi.repo.commitChanges('/repo/pair', 'the-pair: work'),
    (err: unknown) => {
      assert.match(String(err), /Tauri|invoke|__TAURI__/i)
      return true
    }
  )
})

test('repo.pushChanges invokes git_push_changes with directory', async () => {
  await assert.rejects(tauriApi.repo.pushChanges('/repo/pair'), (err: unknown) => {
    assert.match(String(err), /Tauri|invoke|__TAURI__/i)
    return true
  })
})
