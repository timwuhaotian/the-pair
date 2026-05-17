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
    smoke.mentorPromptTemplate.includes('Greeting N/3 received'),
    'smoke preset contains task instructions'
  )
  assert.ok(
    smoke.mentorPromptTemplate.includes('First mentor planning turn'),
    'smoke preset defines the initial mentor planning turn'
  )
  assert.ok(
    smoke.mentorPromptTemplate.includes('Send Greeting 1/3'),
    'smoke preset starts by asking the executor for the first greeting'
  )
  assert.ok(
    smoke.mentorPromptTemplate.includes('continue') &&
      smoke.mentorPromptTemplate.includes('Send Greeting 2/3'),
    'smoke preset keeps greeting 1/3 as an incomplete review'
  )
  assert.ok(
    smoke.mentorPromptTemplate.includes('TASK_COMPLETE'),
    'smoke preset ends only after the third greeting'
  )

  const built = buildSpecFromPreset(smoke, 'check greeting handoff robustness')
  assert.equal(built, smoke.mentorPromptTemplate)
})

test('smoke preset is not part of production preset list', () => {
  const presets = getPresets(false)
  const hasSmokePreset = presets.some((preset) => preset.id === 'dev-smoke-test')
  assert.equal(hasSmokePreset, false)
})
