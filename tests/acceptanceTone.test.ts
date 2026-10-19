import assert from 'node:assert/strict'
import test from 'node:test'

import { acceptanceContentTone, verdictTone } from '../src/renderer/src/lib/acceptanceTone.ts'

const passVerdict = {
  verdict: 'pass',
  risk: 'low',
  evidence: ['npm test passed'],
  summary: 'Ready to ship',
  nextStep: { action: 'finish', instructions: [] }
}

const failVerdict = {
  verdict: 'fail',
  risk: 'high',
  evidence: ['npm run typecheck failed'],
  summary: 'Typecheck still fails',
  nextStep: { action: 'continue', instructions: ['Fix TS2322 in App.tsx'] }
}

function check(name: string, status: 'passed' | 'failed' | 'skipped'): Record<string, unknown> {
  return { name, command: name, status, exitCode: 0, durationMs: 10, summary: status }
}

function record(checks: Array<Record<string, unknown>>, verdict?: object): string {
  return JSON.stringify({ iteration: 1, risk: 'low', summary: 'Checks ran', checks, verdict })
}

test('verdictTone reads pass as done and anything else as running', () => {
  assert.equal(verdictTone('pass'), 'done')
  assert.equal(verdictTone('fail'), 'running')
  assert.equal(verdictTone(null), 'running')
  assert.equal(verdictTone(undefined), 'running')
})

test('a record with a verdict takes its tone from the verdict', () => {
  assert.equal(acceptanceContentTone(record([check('build', 'failed')], passVerdict)), 'done')
  assert.equal(acceptanceContentTone(record([check('build', 'passed')], failVerdict)), 'running')
})

test('a record without a verdict falls back to its checks', () => {
  const mixed = record([check('build', 'passed'), check('test', 'failed')])
  assert.equal(acceptanceContentTone(mixed), 'error')
  const clean = record([check('build', 'passed'), check('lint', 'skipped')])
  assert.equal(acceptanceContentTone(clean), 'done')
})

test('a bare verdict message takes its tone from the verdict', () => {
  assert.equal(acceptanceContentTone(JSON.stringify(passVerdict)), 'done')
  assert.equal(acceptanceContentTone(JSON.stringify(failVerdict)), 'running')
})

test('content that is neither a record nor a verdict stays running', () => {
  assert.equal(acceptanceContentTone('Mentor is still reviewing the diff'), 'running')
})
