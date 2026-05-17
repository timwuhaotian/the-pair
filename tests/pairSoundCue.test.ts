import assert from 'node:assert/strict'
import test from 'node:test'

import {
  resolvePairSoundCue,
  DEFAULT_MANUAL_PAUSE_WINDOW_MS
} from '../src/renderer/src/lib/pairSoundCue.ts'

test('Idle → Finished plays the finish chime', () => {
  assert.equal(resolvePairSoundCue({ prevStatus: 'Idle', nextStatus: 'Finished' }), 'finish')
})

test('Reviewing → Finished plays the finish chime', () => {
  assert.equal(resolvePairSoundCue({ prevStatus: 'Reviewing', nextStatus: 'Finished' }), 'finish')
})

test('Finished → Finished is silent (snapshot rehydration)', () => {
  assert.equal(resolvePairSoundCue({ prevStatus: 'Finished', nextStatus: 'Finished' }), null)
})

test('Reviewing → Awaiting Human Review plays the attention cue', () => {
  assert.equal(
    resolvePairSoundCue({ prevStatus: 'Reviewing', nextStatus: 'Awaiting Human Review' }),
    'attention'
  )
})

test('Awaiting Human Review → Awaiting Human Review is silent', () => {
  assert.equal(
    resolvePairSoundCue({
      prevStatus: 'Awaiting Human Review',
      nextStatus: 'Awaiting Human Review'
    }),
    null
  )
})

test('Mentoring → Error plays the error alert', () => {
  assert.equal(resolvePairSoundCue({ prevStatus: 'Mentoring', nextStatus: 'Error' }), 'error')
})

test('Executing → Paused with no manual pause fires the pause cue (auto-pause)', () => {
  assert.equal(resolvePairSoundCue({ prevStatus: 'Executing', nextStatus: 'Paused' }), 'pause')
})

test('Executing → Paused right after a manual pause is suppressed', () => {
  const now = 1_000_000
  assert.equal(
    resolvePairSoundCue({
      prevStatus: 'Executing',
      nextStatus: 'Paused',
      manualPauseAt: now - 100,
      now
    }),
    null
  )
})

test('Executing → Paused outside the manual suppression window still fires', () => {
  const now = 1_000_000
  assert.equal(
    resolvePairSoundCue({
      prevStatus: 'Executing',
      nextStatus: 'Paused',
      manualPauseAt: now - DEFAULT_MANUAL_PAUSE_WINDOW_MS - 1,
      now
    }),
    'pause'
  )
})

test('Future manualPauseAt (clock skew) does not suppress a legitimate auto-pause', () => {
  const now = 1_000_000
  assert.equal(
    resolvePairSoundCue({
      prevStatus: 'Executing',
      nextStatus: 'Paused',
      manualPauseAt: now + 10_000,
      now
    }),
    'pause'
  )
})

test('undefined prevStatus (pair not yet hydrated) stays silent', () => {
  assert.equal(resolvePairSoundCue({ prevStatus: undefined, nextStatus: 'Finished' }), null)
})

test('undefined nextStatus stays silent', () => {
  assert.equal(resolvePairSoundCue({ prevStatus: 'Mentoring', nextStatus: undefined }), null)
})

test('Idle → Mentoring (normal turn handoff) stays silent', () => {
  assert.equal(resolvePairSoundCue({ prevStatus: 'Idle', nextStatus: 'Mentoring' }), null)
})

test('Paused → Mentoring (resume) stays silent', () => {
  assert.equal(resolvePairSoundCue({ prevStatus: 'Paused', nextStatus: 'Mentoring' }), null)
})
