import assert from 'node:assert/strict'
import test from 'node:test'

import {
  applyPresetTemplate,
  extractPresetTask,
  finalizePresetSpec,
  isPresetTaskMissing,
  presetNeedsTask,
  removePresetTemplate,
  switchPresetTemplate
} from '../src/renderer/src/components/presetSpec.ts'
import { getPresets } from '../src/renderer/src/lib/presetUtils.ts'
import type { PairPreset } from '../src/renderer/src/types.ts'

const presets = getPresets(true)
const bugFix = presets.find((p) => p.id === 'bug-fix') as PairPreset
const feature = presets.find((p) => p.id === 'feature') as PairPreset
const smoke = presets.find((p) => p.id === 'dev-smoke-test') as PairPreset

function occurrences(haystack: string, needle: string): number {
  return haystack.split(needle).length - 1
}

test('selecting a preset on an empty draft inserts the template once with a placeholder task', () => {
  const spec = switchPresetTemplate(null, bugFix, '')
  assert.equal(occurrences(spec, 'You are a meticulous bug investigator'), 1)
  assert.equal(extractPresetTask(bugFix, spec), '')
  assert.equal(isPresetTaskMissing(bugFix, spec), true)
})

test('submitting an edited preset draft never wraps the template twice', () => {
  const draft = switchPresetTemplate(null, bugFix, '')
  const edited = draft.replace(
    'Describe what you want the pair to accomplish...',
    'Fix the login bug'
  )
  assert.equal(extractPresetTask(bugFix, edited), 'Fix the login bug')
  assert.equal(isPresetTaskMissing(bugFix, edited), false)

  const final = finalizePresetSpec(bugFix, bugFix, edited)
  assert.equal(final, edited)
  assert.equal(occurrences(final, 'TASK:'), 1)
  assert.equal(occurrences(final, 'You are a meticulous bug investigator'), 1)
})

test('a selected preset whose template is not applied yet is wrapped exactly once on submit', () => {
  const final = finalizePresetSpec(bugFix, null, 'Fix the login bug')
  assert.equal(final, applyPresetTemplate(bugFix, 'Fix the login bug'))
  assert.equal(occurrences(final, 'TASK:'), 1)
  assert.equal(finalizePresetSpec(null, null, 'plain task'), 'plain task')
})

test('deselecting a preset strips the whole template (prefix and suffix) and keeps the task', () => {
  const spec = applyPresetTemplate(bugFix, 'Fix the login bug')
  assert.equal(removePresetTemplate(bugFix, spec), 'Fix the login bug')
  // Untouched placeholder strips to an empty draft.
  assert.equal(removePresetTemplate(bugFix, applyPresetTemplate(bugFix, '')), '')
})

test('switching presets carries the typed task across instead of nesting templates', () => {
  const bugSpec = applyPresetTemplate(bugFix, 'Fix the login bug')
  const featureSpec = switchPresetTemplate(bugFix, feature, bugSpec)
  assert.equal(extractPresetTask(feature, featureSpec), 'Fix the login bug')
  assert.equal(occurrences(featureSpec, 'You are a meticulous bug investigator'), 0)
  assert.equal(occurrences(featureSpec, 'TASK:'), 1)
})

test('a task typed before picking a preset is wrapped, not discarded', () => {
  const spec = switchPresetTemplate(null, bugFix, '  Fix the login bug  ')
  assert.equal(extractPresetTask(bugFix, spec), 'Fix the login bug')
})

test('an edited template is left alone on deselect', () => {
  const spec = applyPresetTemplate(bugFix, 'Fix it').replace('Be systematic.', 'Be quick.')
  assert.equal(extractPresetTask(bugFix, spec), null)
  assert.equal(removePresetTemplate(bugFix, spec), spec)
  assert.equal(isPresetTaskMissing(bugFix, spec), false)
})

test('templates without a {task} slot never block submission', () => {
  assert.equal(presetNeedsTask(smoke), false)
  const spec = switchPresetTemplate(null, smoke, '')
  assert.equal(isPresetTaskMissing(smoke, spec), false)
  assert.equal(isPresetTaskMissing(null, ''), false)
})
