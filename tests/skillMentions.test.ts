import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  composeFinalSpec,
  selectReferencedSkills,
  type SkillContexts
} from '../src/renderer/src/lib/skillMentions'

function skills(entries: Array<[string, string, string]>): SkillContexts {
  return new Map(entries.map(([name, description, body]) => [name, { description, body }] as const))
}

test('selectReferencedSkills only returns skills still mentioned in the spec', () => {
  const ctx = skills([
    ['tdd', 'red-green-refactor', 'use TDD'],
    ['unused', 'desc', 'body']
  ])
  const spec = 'Refactor module using /tdd discipline.'
  const out = selectReferencedSkills(spec, ctx)
  assert.equal(out.length, 1)
  assert.equal(out[0][0], 'tdd')
})

test('selectReferencedSkills ignores slashes inside paths/URLs', () => {
  const ctx = skills([['tdd', 'd', 'b']])
  // Slash glued to other chars — not at a token boundary.
  const spec = 'See https://example.com/tdd or src/tdd-helper for context.'
  assert.equal(selectReferencedSkills(spec, ctx).length, 0)
})

test('selectReferencedSkills matches /name at start of input or after whitespace', () => {
  const ctx = skills([['tdd', 'd', 'b']])
  assert.equal(selectReferencedSkills('/tdd run it', ctx).length, 1)
  assert.equal(selectReferencedSkills('first then /tdd', ctx).length, 1)
  assert.equal(selectReferencedSkills('(/tdd)', ctx).length, 1)
})

test('composeFinalSpec returns spec unchanged when nothing referenced', () => {
  const spec = 'plain task'
  const out = composeFinalSpec(spec, new Map(), new Map(), 'claude')
  assert.equal(out, spec)
})

test('composeFinalSpec injects a reference directive for the claude executor', () => {
  const ctx = skills([
    ['tdd', 'red-green-refactor', 'full body that should NOT appear'],
    ['polish', 'finishing touches', 'also hidden']
  ])
  const spec = 'Use /tdd and then /polish before shipping.'
  const out = composeFinalSpec(spec, new Map(), ctx, 'claude')

  assert.ok(out.startsWith('--- SKILLS REQUESTED ---'))
  assert.ok(out.includes('/tdd'))
  assert.ok(out.includes('/polish'))
  // Claude provider must NOT inline the body — its Skill tool fetches it.
  assert.ok(!out.includes('full body'))
  assert.ok(out.endsWith('--- TASK ---\n' + spec))
})

test('composeFinalSpec inlines skill bodies for non-claude executors', () => {
  const ctx = skills([['tdd', 'red-green', 'BODY_CONTENT_HERE']])
  const spec = '/tdd build the thing.'
  for (const provider of ['opencode', 'codex', 'gemini', 'kimi'] as const) {
    const out = composeFinalSpec(spec, new Map(), ctx, provider)
    assert.ok(out.startsWith('--- SKILLS LOADED ---'), `${provider} starts with SKILLS LOADED`)
    assert.ok(out.includes('## /tdd'), `${provider} contains skill heading`)
    assert.ok(out.includes('BODY_CONTENT_HERE'), `${provider} inlines body`)
    assert.ok(out.endsWith('--- TASK ---\n' + spec))
  }
})

test('composeFinalSpec orders skills before files before the task body', () => {
  const ctx = skills([['tdd', 'd', 'SKILL_BODY']])
  const files = new Map([['src/a.ts', 'FILE_BODY']])
  const spec = '/tdd update @src/a.ts now.'
  const out = composeFinalSpec(spec, files, ctx, 'opencode')

  const skillIdx = out.indexOf('SKILLS LOADED')
  const fileIdx = out.indexOf('REFERENCED FILES')
  const taskIdx = out.indexOf('--- TASK ---')
  assert.ok(skillIdx >= 0 && fileIdx > skillIdx && taskIdx > fileIdx)
})

test('composeFinalSpec drops skills the user deleted before submitting', () => {
  const ctx = skills([
    ['keep', 'd1', 'KEEP_BODY'],
    ['dropped', 'd2', 'DROP_BODY']
  ])
  const spec = 'Only /keep should remain.'
  const out = composeFinalSpec(spec, new Map(), ctx, 'opencode')
  assert.ok(out.includes('KEEP_BODY'))
  assert.ok(!out.includes('DROP_BODY'))
})
