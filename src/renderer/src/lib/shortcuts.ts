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
