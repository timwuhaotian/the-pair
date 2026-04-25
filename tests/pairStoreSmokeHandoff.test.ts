import assert from 'node:assert/strict'
import test from 'node:test'
import { buildSpecFromPreset, getPresets } from '../src/renderer/src/lib/presetUtils.ts'

test('smoke preset is a self-contained spec template', () => {
  const presets = getPresets(true)
  const smoke = presets.find((preset) => preset.id === 'dev-smoke-test')
  assert.ok(smoke, 'dev smoke preset exists in dev mode')
  assert.ok(
    !smoke.mentorPromptTemplate.includes('{task}'),
    'smoke preset has no {task} placeholder'
  )
  assert.ok(
    smoke.mentorPromptTemplate.includes('TASK_COMPLETE'),
    'smoke preset contains task instructions'
  )

  const built = buildSpecFromPreset(smoke, 'check greeting handoff robustness')
  assert.equal(built, smoke.mentorPromptTemplate)
})

test('smoke preset is not part of production preset list', () => {
  const presets = getPresets(false)
  const hasSmokePreset = presets.some((preset) => preset.id === 'dev-smoke-test')
  assert.equal(hasSmokePreset, false)
})
