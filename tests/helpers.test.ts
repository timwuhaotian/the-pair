import assert from 'node:assert/strict'
import test from 'node:test'

import { isAgentExecuting } from '../src/renderer/src/lib/helpers.ts'

test('isAgentExecuting returns true for thinking', () => {
  assert.equal(isAgentExecuting('thinking'), true)
})

test('isAgentExecuting returns true for using_tools', () => {
  assert.equal(isAgentExecuting('using_tools'), true)
})

test('isAgentExecuting returns true for responding', () => {
  assert.equal(isAgentExecuting('responding'), true)
})

test('isAgentExecuting returns false for idle', () => {
  assert.equal(isAgentExecuting('idle'), false)
})

test('isAgentExecuting returns false for error', () => {
  assert.equal(isAgentExecuting('error'), false)
})

test('isAgentExecuting returns false for done', () => {
  assert.equal(isAgentExecuting('done'), false)
})

test('isAgentExecuting returns false for stalled', () => {
  assert.equal(isAgentExecuting('stalled'), false)
})

test('isAgentExecuting returns false for empty string', () => {
  assert.equal(isAgentExecuting(''), false)
})

test('isAgentExecuting is case-sensitive (THINKING does not match)', () => {
  assert.equal(isAgentExecuting('THINKING'), false)
})
