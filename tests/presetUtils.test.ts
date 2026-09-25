import assert from 'node:assert/strict'
import test from 'node:test'

import {
  applyPresetTemplate,
  stripTemplate,
  stripPresetTemplate,
  specHasPresetTemplate,
  buildSpecFromPreset,
  getPresets
} from '../src/renderer/src/lib/presetUtils.ts'
import type { PairPreset } from '../src/renderer/src/types.ts'

// ── stripTemplate ──────────────────────────────────────

test('stripTemplate extracts task after TASK: marker', () => {
  const result = stripTemplate('ROLE: MENTOR.\nblah\nTASK: do the thing')
  assert.equal(result, 'do the thing')
})

test('stripTemplate returns unchanged when no TASK marker', () => {
  assert.equal(stripTemplate('no task marker here'), 'no task marker here')
})

// ── buildSpecFromPreset ────────────────────────────────

test('buildSpecFromPerset replaces {task} placeholder', () => {
  const preset: PairPreset = {
    id: 'test',
    name: 'Test',
    description: 'test',
    icon: 'Bug',
    mentorPromptTemplate: 'Instructions for {task} end here',
    executorPromptTemplate: '',
    recommendedSkills: []
  }
  const result = buildSpecFromPreset(preset, 'fix the login bug')
  assert.ok(result.includes('fix the login bug'))
  assert.ok(!result.includes('{task}'))
})

test('buildSpecFromPreset returns template unchanged when no placeholder', () => {
  const preset: PairPreset = {
    id: 'test',
    name: 'Test',
    description: 'test',
    icon: 'Bug',
    mentorPromptTemplate: 'Static instructions with no placeholder',
    executorPromptTemplate: '',
    recommendedSkills: []
  }
  const result = buildSpecFromPreset(preset, 'do something')
  assert.equal(result, 'Static instructions with no placeholder')
})

test('buildSpecFromPreset keeps $ sequences in the task literal', () => {
  const preset = getPresets(false).find((p) => p.id === 'bug-fix')!
  for (const task of [`IFS=$'\\n' in deploy.sh`, 'use $$ instead of $&', 'cost is $` and $1']) {
    const spec = buildSpecFromPreset(preset, task)
    assert.ok(spec.includes(`TASK:\n${task}\n`), task)
    assert.ok(!spec.includes('{task}'), task)
  }
})

// ── preset template detection / stripping ──────────────

const bugFix = getPresets(false).find((p) => p.id === 'bug-fix')!

test('specHasPresetTemplate detects a textarea pre-filled with the template', () => {
  const prefilled = buildSpecFromPreset(bugFix, '')
  assert.equal(specHasPresetTemplate(prefilled, bugFix), true)
  const edited = prefilled.replace(
    'Describe what you want the pair to accomplish...',
    'Fix the login bug'
  )
  assert.equal(specHasPresetTemplate(edited, bugFix), true)
  assert.equal(specHasPresetTemplate('Fix the login bug', bugFix), false)
})

test('applyPresetTemplate wraps a bare task once and never twice', () => {
  const edited = buildSpecFromPreset(bugFix, '').replace(
    'Describe what you want the pair to accomplish...',
    'Fix the login bug'
  )
  const applied = applyPresetTemplate(bugFix, edited)
  assert.equal(applied, edited)
  assert.equal(applied.split('You are a meticulous bug investigator').length - 1, 1)

  const wrapped = applyPresetTemplate(bugFix, 'Fix the login bug')
  assert.equal(wrapped, buildSpecFromPreset(bugFix, 'Fix the login bug'))
})

test('stripPresetTemplate returns the task without the template preamble or postamble', () => {
  const spec = buildSpecFromPreset(bugFix, 'Fix the login bug')
  assert.equal(stripPresetTemplate(spec, bugFix), 'Fix the login bug')
  // Untouched placeholder text is not kept as a task.
  assert.equal(stripPresetTemplate(buildSpecFromPreset(bugFix, ''), bugFix), '')
  // Text the user added around the wrapper survives.
  assert.equal(
    stripPresetTemplate(`Context first.\n\n${spec}\n\nAlso add a test.`, bugFix),
    'Context first.\n\nFix the login bug\n\nAlso add a test.'
  )
  // Not wrapped: unchanged.
  assert.equal(stripPresetTemplate('plain task', bugFix), 'plain task')
})

test('stripTemplate removes any known preset wrapper, postamble included', () => {
  for (const preset of getPresets(false)) {
    const spec = buildSpecFromPreset(preset, 'Harden the API client')
    assert.equal(stripTemplate(spec), 'Harden the API client', preset.id)
  }
  const smoke = getPresets(true).find((p) => p.id === 'dev-smoke-test')!
  assert.equal(stripTemplate(buildSpecFromPreset(smoke, '')), '')
})

// ── getPresets ─────────────────────────────────────────

test('getPresets(false) returns at least 4 standard presets', () => {
  const presets = getPresets(false)
  assert.ok(presets.length >= 4)
  const ids = presets.map((p) => p.id)
  assert.ok(ids.includes('bug-fix'))
  assert.ok(ids.includes('refactor'))
  assert.ok(ids.includes('feature'))
  assert.ok(ids.includes('hardening'))
})

test('getPresets(true) includes dev-smoke-test preset', () => {
  const presets = getPresets(true)
  const ids = presets.map((p) => p.id)
  assert.ok(ids.includes('dev-smoke-test'))
})
