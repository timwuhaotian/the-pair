import { useEffect, useLayoutEffect, useRef } from 'react'
import type { ShortcutDef } from '../lib/shortcuts'
import { matchesShortcut, isInputFocused, isCommandShortcut } from '../lib/shortcuts'

export function useShortcuts(shortcuts: ShortcutDef[]): void {
  // The list is rebuilt whenever the selected pair changes (every state event
  // while it runs); read the latest through a ref and bind the listener once.
  const shortcutsRef = useRef(shortcuts)
  useLayoutEffect(() => {
    shortcutsRef.current = shortcuts
  })

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent): void => {
      const inputFocused = isInputFocused(e)

      for (const shortcut of shortcutsRef.current) {
        // Inside a text field only command-modified shortcuts fire (Cmd+K,
        // Cmd+1…); bare keys are typing, not commands.
        if (inputFocused && !isCommandShortcut(e, shortcut)) continue
        if (matchesShortcut(e, shortcut)) {
          if (shortcut.condition && !shortcut.condition()) continue
          e.preventDefault()
          shortcut.handler()
          break
        }
      }
    }

    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [])
}
