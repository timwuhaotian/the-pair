import assert from 'node:assert/strict'
import test from 'node:test'

import { shouldIgnoreHandoffEvent } from '../src/renderer/src/lib/handoffGuard.ts'

test('backend finished handoff is ignored even when local pair state is stale', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Executing',
      backendStatus: 'Finished'
    }),
    true
  )
})

test('active handoff is allowed when neither state is finished', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Executing',
      backendStatus: 'Executing'
    }),
    false
  )
})

test('Paused pairStatus blocks handoff', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Paused',
      backendStatus: 'Executing'
    }),
    true
  )
})

test('Paused backendStatus blocks handoff', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Executing',
      backendStatus: 'Paused'
    }),
    true
  )
})

test('Error pairStatus blocks handoff', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Error',
      backendStatus: 'Executing'
    }),
    true
  )
})

test('Error backendStatus blocks handoff', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Executing',
      backendStatus: 'Error'
    }),
    true
  )
})

test('both Paused blocks handoff', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Paused',
      backendStatus: 'Paused'
    }),
    true
  )
})

test('both Error blocks handoff', () => {
  assert.equal(
    shouldIgnoreHandoffEvent({
      pairStatus: 'Error',
      backendStatus: 'Error'
    }),
    true
  )
})
