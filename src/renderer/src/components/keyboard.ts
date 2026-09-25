/**
 * Keyboard helpers shared by components that submit, select, or close on a key
 * press. Kept free of React imports so they can be unit-tested under node.
 */

/** Minimal shape shared by DOM `KeyboardEvent` and React's synthetic event. */
export interface KeyEventLike {
  key: string
  keyCode?: number
  isComposing?: boolean
  defaultPrevented?: boolean
  nativeEvent?: { isComposing?: boolean; keyCode?: number }
}

/** keyCode browsers report for a key press consumed by an input method editor. */
const IME_PROCESS_KEY_CODE = 229

/**
 * True when the key event belongs to an IME composition (e.g. the Enter that
 * confirms a kana→kanji candidate). WebKit — the macOS Tauri webview — ends
 * the composition *before* dispatching that Enter keydown, so `isComposing`
 * is already false there; only `keyCode === 229` still identifies it.
 */
export function isImeKeyEvent(e: KeyEventLike): boolean {
  return Boolean(
    e.isComposing ||
    e.nativeEvent?.isComposing ||
    e.keyCode === IME_PROCESS_KEY_CODE ||
    e.nativeEvent?.keyCode === IME_PROCESS_KEY_CODE
  )
}

export interface CompositionTracker {
  /** True while an IME composition is active (and for the rest of the task
   * in which it ended, so the confirming Enter keydown still counts). */
  isComposing: () => boolean
  onCompositionStart: () => void
  onCompositionEnd: () => void
  dispose: () => void
}

/**
 * Tracks IME composition via compositionstart/compositionend. The "composing"
 * flag is cleared on the next macrotask after compositionend: WebKit fires
 * compositionend and then the confirming Enter keydown synchronously, so the
 * keydown still sees the composition as active.
 */
export function createCompositionTracker(
  schedule: (callback: () => void) => ReturnType<typeof setTimeout> = (callback) =>
    setTimeout(callback, 0),
  cancel: (handle: ReturnType<typeof setTimeout>) => void = (handle) => clearTimeout(handle)
): CompositionTracker {
  let composing = false
  let pending: ReturnType<typeof setTimeout> | null = null

  const clearPending = (): void => {
    if (pending !== null) {
      cancel(pending)
      pending = null
    }
  }

  return {
    isComposing: () => composing,
    onCompositionStart: () => {
      clearPending()
      composing = true
    },
    onCompositionEnd: () => {
      clearPending()
      pending = schedule(() => {
        pending = null
        composing = false
      })
    },
    dispose: clearPending
  }
}

/** Plain Enter (no Shift) that is not part of an IME composition. */
export function isSubmitEnter(
  e: KeyEventLike & { shiftKey?: boolean },
  composing = false
): boolean {
  return e.key === 'Enter' && !e.shiftKey && !composing && !isImeKeyEvent(e)
}

/**
 * Whether a document-level Escape listener of a modal should close it. Escape
 * that a nested popover/picker already handled (defaultPrevented) or that
 * cancels an IME composition must leave the modal — and its draft — alone.
 */
export function shouldCloseOnEscape(e: KeyEventLike): boolean {
  return e.key === 'Escape' && !e.defaultPrevented && !isImeKeyEvent(e)
}

/**
 * keydown handler for single-line inputs inside a form: blocks the implicit
 * form submission WebKit performs for the Enter that confirms an IME candidate.
 */
export function preventImeEnterSubmit(e: KeyEventLike & { preventDefault: () => void }): void {
  if (e.key === 'Enter' && isImeKeyEvent(e)) e.preventDefault()
}
