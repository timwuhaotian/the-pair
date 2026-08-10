import assert from 'node:assert/strict'
import test from 'node:test'

import { isPairActive, isPairBusy } from '../src/renderer/src/lib/pairStatus.ts'

type PairStatus =
  | 'Idle'
  | 'Mentoring'
  | 'Executing'
  | 'Reviewing'
  | 'Paused'
  | 'Awaiting Human Review'
  | 'Error'
  | 'Finished'

const allStatuses: PairStatus[] = [
  'Idle',
  'Mentoring',
  'Executing',
  'Reviewing',
  'Paused',
  'Awaiting Human Review',
  'Error',
  'Finished'
]

// ── isPairActive ───────────────────────────────────────

test('isPairActive returns true for Mentoring', () => {
  assert.equal(isPairActive('Mentoring'), true)
})

test('isPairActive returns true for Executing', () => {
  assert.equal(isPairActive('Executing'), true)
})

test('isPairActive returns true for Reviewing', () => {
  assert.equal(isPairActive('Reviewing'), true)
})

test('isPairActive returns false for Idle', () => {
  assert.equal(isPairActive('Idle'), false)
})

test('isPairActive returns false for Paused', () => {
  assert.equal(isPairActive('Paused'), false)
})

test('isPairActive returns false for Awaiting Human Review', () => {
  assert.equal(isPairActive('Awaiting Human Review'), false)
})

test('isPairActive returns false for Error', () => {
  assert.equal(isPairActive('Error'), false)
})

test('isPairActive returns false for Finished', () => {
  assert.equal(isPairActive('Finished'), false)
})

// ── isPairBusy ─────────────────────────────────────────

test('isPairBusy returns true for Mentoring', () => {
  assert.equal(isPairBusy('Mentoring'), true)
})

test('isPairBusy returns true for Executing', () => {
  assert.equal(isPairBusy('Executing'), true)
})

test('isPairBusy returns true for Reviewing', () => {
  assert.equal(isPairBusy('Reviewing'), true)
})

test('isPairBusy returns true for Awaiting Human Review', () => {
  assert.equal(isPairBusy('Awaiting Human Review'), true)
})

test('isPairBusy returns false for Idle', () => {
  assert.equal(isPairBusy('Idle'), false)
})

test('isPairBusy returns false for Paused', () => {
  assert.equal(isPairBusy('Paused'), false)
})

test('isPairBusy returns false for Error', () => {
  assert.equal(isPairBusy('Error'), false)
})

test('isPairBusy returns false for Finished', () => {
  assert.equal(isPairBusy('Finished'), false)
})

// ── Truth table summary ────────────────────────────────

test('isPairActive truth table covers all 8 statuses', () => {
  const expected: Record<PairStatus, boolean> = {
    Idle: false,
    Mentoring: true,
    Executing: true,
    Reviewing: true,
    Paused: false,
    'Awaiting Human Review': false,
    Error: false,
    Finished: false
  }
  for (const status of allStatuses) {
    assert.equal(isPairActive(status), expected[status], `isPairActive(${status})`)
  }
})

test('isPairBusy truth table covers all 8 statuses', () => {
  const expected: Record<PairStatus, boolean> = {
    Idle: false,
    Mentoring: true,
    Executing: true,
    Reviewing: true,
    Paused: false,
    'Awaiting Human Review': true,
    Error: false,
    Finished: false
  }
  for (const status of allStatuses) {
    assert.equal(isPairBusy(status), expected[status], `isPairBusy(${status})`)
  }
})
