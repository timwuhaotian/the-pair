import assert from 'node:assert/strict'
import test from 'node:test'

import {
  cn,
  extractErrorMessage,
  formatIterations,
  stripSystemPrompt
} from '../src/renderer/src/lib/utils.ts'

test('cn merges conflicting Tailwind classes with the later utility winning', () => {
  assert.equal(cn('px-2', 'px-4', 'text-sm'), 'px-4 text-sm')
})

test('cn ignores falsy fragments while preserving valid class names', () => {
  assert.equal(cn('flex', false, null, undefined, 'items-center'), 'flex items-center')
})

// ── extractErrorMessage ────────────────────────────────

test('extractErrorMessage extracts message from Error instance', () => {
  assert.equal(extractErrorMessage(new Error('boom'), 'fallback'), 'boom')
})

test('extractErrorMessage returns string errors directly', () => {
  assert.equal(extractErrorMessage('string error', 'fallback'), 'string error')
})

test('extractErrorMessage extracts message from objects with message property', () => {
  assert.equal(extractErrorMessage({ message: 'obj error' }, 'fallback'), 'obj error')
})

test('extractErrorMessage returns fallback for non-error primitives', () => {
  assert.equal(extractErrorMessage(42, 'fallback'), 'fallback')
})

test('extractErrorMessage returns fallback for null', () => {
  assert.equal(extractErrorMessage(null, 'fallback'), 'fallback')
})

// ── formatIterations ───────────────────────────────────

test('formatIterations formats finite budget', () => {
  assert.equal(formatIterations(3, 6), '3/6')
})

test('formatIterations formats zero max as infinity', () => {
  assert.equal(formatIterations(5, 0), '5/∞')
})

test('formatIterations formats undefined max as infinity', () => {
  assert.equal(formatIterations(5, undefined), '5/∞')
})

test('formatIterations formats negative max as infinity', () => {
  assert.equal(formatIterations(5, -1), '5/∞')
})

// ── stripSystemPrompt ──────────────────────────────────

test('stripSystemPrompt returns unchanged content without system prompt', () => {
  assert.equal(stripSystemPrompt('hello world'), 'hello world')
})

test('stripSystemPrompt strips system prompt and extracts user task', () => {
  const result = stripSystemPrompt('ROLE: MENTOR.\nblah\nTASK: do the thing')
  assert.ok(result.includes('do the thing'))
})

test('stripSystemPrompt extracts user content after role headers', () => {
  const input =
    '### ROLE: MENTOR\nYou are a mentor.\nYour mission is to guide.\n\nActual user content here'
  const result = stripSystemPrompt(input)
  assert.ok(result.length > 0)
  assert.ok(result.includes('Actual user content here'))
})
