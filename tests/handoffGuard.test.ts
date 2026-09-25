import assert from 'node:assert/strict'
import test from 'node:test'

import {
  isHandoffIgnoredError,
  shouldIgnoreHandoffEvent
} from '../src/renderer/src/lib/handoffGuard.ts'

test('backend finished handoff is ignored even when local pair state is stale', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Executing',
      backendStatus: 'finished'
    }),
    true
  )
})

test('active handoff is allowed when neither state is finished', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Executing',
      backendStatus: 'executing'
    }),
    false
  )
})

test('Paused pairStatus blocks handoff', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Paused',
      backendStatus: 'executing'
    }),
    true
  )
})

test('Paused backendStatus blocks handoff', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Executing',
      backendStatus: 'paused'
    }),
    true
  )
})

test('Error pairStatus blocks handoff', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Error',
      backendStatus: 'executing'
    }),
    true
  )
})

test('Error backendStatus blocks handoff', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Executing',
      backendStatus: 'error'
    }),
    true
  )
})

test('both Paused blocks handoff', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Paused',
      backendStatus: 'paused'
    }),
    true
  )
})

test('both Error blocks handoff', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Error',
      backendStatus: 'error'
    }),
    true
  )
})

test('backend statuses are compared case-insensitively (pair_get_state sends kebab-case)', () => {
  for (const backendStatus of ['finished', 'paused', 'error', 'Finished', 'PAUSED']) {
    assert.equal(
      shouldIgnoreHandoffEvent({ pairStatus: 'Executing', backendStatus }),
      true,
      backendStatus
    )
  }
  for (const backendStatus of ['mentoring', 'executing', 'reviewing', 'idle', undefined, null]) {
    assert.equal(
      shouldIgnoreHandoffEvent({ pairStatus: 'Executing', backendStatus }),
      false,
      String(backendStatus)
    )
  }
})

test('Awaiting Human Review blocks handoff in either spelling', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({ pairStatus: 'Executing', backendStatus: 'awaiting-human-review' }),
    true
  )
  assert.equal(
    shouldIgnoreHandoffEvent({ pairStatus: 'Executing', backendStatus: 'Awaiting Human Review' }),
    true
  )
  assert.equal(
    shouldIgnoreHandoffEvent({ pairStatus: 'Awaiting Human Review', backendStatus: undefined }),
    true
  )
})

test('isHandoffIgnoredError recognizes the backend HANDOFF_IGNORED rejection', () => {
  assert.equal(isHandoffIgnoredError('HANDOFF_IGNORED: pair is paused'), true)
  assert.equal(isHandoffIgnoredError(new Error('HANDOFF_IGNORED: pair is finished')), true)
  assert.equal(isHandoffIgnoredError({ message: 'HANDOFF_IGNORED: pair is error' }), true)
  assert.equal(isHandoffIgnoredError(new Error('Pair not found')), false)
  assert.equal(isHandoffIgnoredError(undefined), false)
})
