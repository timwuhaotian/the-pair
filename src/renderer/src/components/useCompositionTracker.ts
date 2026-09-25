import { useEffect, useState } from 'react'
import { createCompositionTracker, type CompositionTracker } from './keyboard'

/**
 * Stable per-component IME composition tracker. Wire `onCompositionStart` /
 * `onCompositionEnd` to the input, then check `isComposing()` (together with
 * `isImeKeyEvent`) before treating Enter as submit/select.
 */
export function useCompositionTracker(): CompositionTracker {
  const [tracker] = useState(() => createCompositionTracker())
  useEffect(() => () => tracker.dispose(), [tracker])
  return tracker
}
