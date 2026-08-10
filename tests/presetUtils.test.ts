import assert from 'node:assert/strict'
import test from 'node:test'

import {
  stripTemplate,
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
