export interface ShortcutDef {
  key: string
  modifiers: string[]
  handler: () => void
  description: string
  condition?: () => boolean
}

const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/.test(navigator.platform)

export const modifierLabel = isMac ? '\u2318' : 'Ctrl'
export const shiftLabel = isMac ? '\u21E7' : 'Shift'

export function matchesShortcut(e: KeyboardEvent, shortcut: ShortcutDef): boolean {
  if (e.key.toLowerCase() !== shortcut.key.toLowerCase()) return false

  const hasMeta = e.metaKey
  const hasCtrl = e.ctrlKey
  const hasShift = e.shiftKey
  const hasAlt = e.altKey

  const expectMeta = shortcut.modifiers.includes('meta') || shortcut.modifiers.includes('cmd')
  const expectCtrl = shortcut.modifiers.includes('ctrl')
  const expectShift = shortcut.modifiers.includes('shift')
  const expectAlt = shortcut.modifiers.includes('alt')

  if (expectMeta !== hasMeta) return false
  if (expectCtrl !== hasCtrl) return false
  if (expectShift !== hasShift) return false
  if (expectAlt !== hasAlt) return false

  return true
}

/**
 * True when a shortcut should fire even while a text field has focus: it
 * carries a command/control modifier, so it cannot be mistaken for typing.
 * Bare-key shortcuts (letters, `?`) stay suppressed inside inputs.
 */
export function isCommandShortcut(e: KeyboardEvent, shortcut: ShortcutDef): boolean {
  return (
    e.metaKey || e.ctrlKey || shortcut.modifiers.some((m) => ['meta', 'cmd', 'ctrl'].includes(m))
  )
}

/** Modal surfaces rendered by App that can sit over the console when Cmd/Ctrl+F fires. */
export interface ConsoleSearchBlockers {
  createPairOpen: boolean
  assignTaskOpen: boolean
  pairSettingsOpen: boolean
  paletteOpen: boolean
  shortcutsOpen: boolean
  confirmDeletePairOpen: boolean
  confirmClearSessionOpen: boolean
  updateModalOpen: boolean
}

/**
 * True when the transcript-search shortcut may dispatch. Every aria-modal
 * dialog App renders over the console blocks it — the console listener would
 * flush the search bar open and rAF-focus its input behind the dialog —
 * including the ConfirmModals and update modal, which the console-side
 * guards structurally cannot see.
 */
export function canOpenConsoleSearch(
  hasSelectedPair: boolean,
  blockers: ConsoleSearchBlockers
): boolean {
  if (!hasSelectedPair) return false
  for (const open of Object.values(blockers)) {
    if (open) return false
  }
  return true
}

export function isInputFocused(e: KeyboardEvent): boolean {
  const target = e.target as HTMLElement
  if (!target) return false
  const tag = target.tagName.toLowerCase()
  if (tag === 'input' || tag === 'textarea') return true
  if (target.isContentEditable) return true
  return false
}

/**
 * Break a shortcut into display tokens, e.g. ['\u2318', '\u21e7', 'P'].
 * Symbol keys like "?" already imply Shift on most layouts, so the standalone
 * \u21e7 token is dropped for them to avoid redundant labels like "\u21e7?".
 */
export function formatShortcutParts(shortcut: ShortcutDef): string[] {
  const isSymbolKey = shortcut.key.length === 1 && !/[a-z0-9]/i.test(shortcut.key)
  const parts: string[] = []
  for (const mod of shortcut.modifiers) {
    if (mod === 'meta' || mod === 'cmd' || mod === 'ctrl') parts.push(modifierLabel)
    else if (mod === 'shift') {
      if (!isSymbolKey) parts.push(shiftLabel)
    } else if (mod === 'alt') parts.push(isMac ? '\u2325' : 'Alt')
  }
  const keyLabel = shortcut.key.length === 1 ? shortcut.key.toUpperCase() : shortcut.key
  parts.push(keyLabel)
  return parts
}

export function formatShortcutLabel(shortcut: ShortcutDef): string {
  return formatShortcutParts(shortcut).join('')
}
