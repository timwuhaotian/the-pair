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

export function formatShortcutLabel(shortcut: ShortcutDef): string {
  const parts: string[] = []
  for (const mod of shortcut.modifiers) {
    if (mod === 'meta' || mod === 'cmd') parts.push(modifierLabel)
    else if (mod === 'ctrl') parts.push(modifierLabel)
    else if (mod === 'shift') parts.push(shiftLabel)
    else if (mod === 'alt') parts.push(isMac ? '\u2325' : 'Alt')
  }
  const keyLabel = shortcut.key.length === 1 ? shortcut.key.toUpperCase() : shortcut.key
  parts.push(keyLabel)
  return parts.join('')
}
