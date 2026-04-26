import { useEffect } from 'react'
import type { ShortcutDef } from '../lib/shortcuts'
import { matchesShortcut, isInputFocused } from '../lib/shortcuts'

export function useShortcuts(shortcuts: ShortcutDef[]): void {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent): void => {
      if (isInputFocused(e)) return

      for (const shortcut of shortcuts) {
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
  }, [shortcuts])
}
