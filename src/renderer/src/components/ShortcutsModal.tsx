import React from 'react'
import { useTranslation } from 'react-i18next'
import { GlassModal } from './ui/GlassModal'
import { formatShortcutParts, type ShortcutDef } from '../lib/shortcuts'

interface ShortcutsModalProps {
  isOpen: boolean
  onClose: () => void
  shortcuts: ShortcutDef[]
}

/**
 * Keyboard cheat sheet. Renders the same {@link ShortcutDef} list that powers
 * the global handlers, so the overlay can never drift from what actually fires.
 */
export function ShortcutsModal({
  isOpen,
  onClose,
  shortcuts
}: ShortcutsModalProps): React.ReactNode {
  const { t } = useTranslation()

  return (
    <GlassModal isOpen={isOpen} onClose={onClose} title={t('shortcuts.title')} className="max-w-md">
      <ul className="flex flex-col divide-y divide-border">
        {shortcuts.map((shortcut, index) => (
          <li
            key={`${shortcut.key}-${index}`}
            className="flex items-center justify-between gap-4 py-2.5 first:pt-0 last:pb-0"
          >
            <span className="text-[12px] text-foreground">{shortcut.description}</span>
            <span className="flex shrink-0 items-center gap-1">
              {formatShortcutParts(shortcut).map((part, partIndex) => (
                <kbd
                  key={partIndex}
                  className="inline-flex min-w-[1.7em] items-center justify-center rounded-sm border border-border bg-foreground/[0.04] px-1.5 py-0.5 font-mono text-[11px] leading-none text-muted-foreground"
                >
                  {part}
                </kbd>
              ))}
            </span>
          </li>
        ))}
      </ul>
    </GlassModal>
  )
}
