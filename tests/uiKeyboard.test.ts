import assert from 'node:assert/strict'
import test from 'node:test'

import {
  createCompositionTracker,
  isImeKeyEvent,
  isSubmitEnter,
  preventImeEnterSubmit,
  shouldCloseOnEscape
} from '../src/renderer/src/components/keyboard.ts'

test('WebKit IME-confirming Enter (isComposing false, keyCode 229) is recognised as IME', () => {
  assert.equal(isImeKeyEvent({ key: 'Enter', keyCode: 229, isComposing: false }), true)
  assert.equal(
    isImeKeyEvent({
      key: 'Enter',
      keyCode: 229,
      nativeEvent: { isComposing: false, keyCode: 229 }
    }),
    true
  )
  assert.equal(isImeKeyEvent({ key: 'Enter', nativeEvent: { isComposing: true } }), true)
  assert.equal(isImeKeyEvent({ key: 'Enter', keyCode: 13, isComposing: false }), false)
})

test('isSubmitEnter only accepts a plain, non-IME Enter', () => {
  assert.equal(isSubmitEnter({ key: 'Enter', keyCode: 13 }), true)
  assert.equal(isSubmitEnter({ key: 'Enter', keyCode: 13, shiftKey: true }), false)
  assert.equal(isSubmitEnter({ key: 'Enter', keyCode: 229 }), false)
  assert.equal(isSubmitEnter({ key: 'Enter', keyCode: 13 }, true), false)
  assert.equal(isSubmitEnter({ key: 'a', keyCode: 65 }), false)
})

test('composition tracker stays "composing" until the task after compositionend', () => {
  const scheduled: Array<() => void> = []
  let cancelled = 0
  const tracker = createCompositionTracker(
    (callback) => {
      scheduled.push(callback)
      return scheduled.length as unknown as ReturnType<typeof setTimeout>
    },
    () => {
      cancelled += 1
    }
  )

  assert.equal(tracker.isComposing(), false)
  tracker.onCompositionStart()
  assert.equal(tracker.isComposing(), true)
  tracker.onCompositionEnd()
  // WebKit dispatches the confirming Enter keydown right after compositionend.
  assert.equal(tracker.isComposing(), true)
  scheduled.shift()?.()
  assert.equal(tracker.isComposing(), false)

  // A new composition starting before the deferred reset cancels it.
  tracker.onCompositionStart()
  tracker.onCompositionEnd()
  tracker.onCompositionStart()
  assert.equal(cancelled, 1)
  assert.equal(tracker.isComposing(), true)
})

test('modal Escape handlers ignore Escape already handled by a popover or used by an IME', () => {
  assert.equal(shouldCloseOnEscape({ key: 'Escape', defaultPrevented: false }), true)
  assert.equal(shouldCloseOnEscape({ key: 'Escape', defaultPrevented: true }), false)
  assert.equal(shouldCloseOnEscape({ key: 'Escape', keyCode: 229 }), false)
  assert.equal(shouldCloseOnEscape({ key: 'Enter' }), false)
})

test('preventImeEnterSubmit blocks implicit form submission only for the IME Enter', () => {
  let prevented = 0
  const preventDefault = (): void => {
    prevented += 1
  }
  preventImeEnterSubmit({ key: 'Enter', keyCode: 229, preventDefault })
  assert.equal(prevented, 1)
  preventImeEnterSubmit({ key: 'Enter', keyCode: 13, preventDefault })
  preventImeEnterSubmit({ key: 'a', keyCode: 229, preventDefault })
  assert.equal(prevented, 1)
})
