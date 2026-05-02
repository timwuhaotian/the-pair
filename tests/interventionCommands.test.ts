import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

import { getMissingCommandInput } from '../src/renderer/src/lib/interventions.ts'

const libSource = await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8')
const pairManagerSource = await readFile(
  new URL('../src-tauri/src/pair_manager.rs', import.meta.url),
  'utf8'
)

test('intervention commands are registered with Tauri', () => {
  for (const command of [
    'send_intervention',
    'add_annotation',
    'resolve_annotation',
    'get_preferences_summary'
  ]) {
    assert.match(libSource, new RegExp(`pair_manager::${command}`))
    assert.match(pairManagerSource, new RegExp(`pub (?:async )?fn ${command}\\b`))
  }
})

test('command validation requires target and content before execution', () => {
  assert.equal(getMissingCommandInput({ hasTarget: true }, '', ''), 'target')
  assert.equal(getMissingCommandInput({ hasContent: true }, '', ''), 'content')
  assert.equal(getMissingCommandInput({}, '', ''), null)
})

test('send_intervention only reports durable success after applying the turn', () => {
  const sendStart = pairManagerSource.indexOf('pub async fn send_intervention')
  const nextCommand = pairManagerSource.indexOf('#[tauri::command]', sendStart + 1)
  const sendBody = pairManagerSource.slice(sendStart, nextCommand)

  assert.ok(sendBody.indexOf('.trigger_turn(') < sendBody.indexOf('InterventionStatus::Applied'))
  assert.doesNotMatch(sendBody, /let _ = persist_current_pair_snapshot/)
})
