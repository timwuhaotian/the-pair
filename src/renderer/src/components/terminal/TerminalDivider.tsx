import React from 'react'
import { cn } from '../../lib/utils'

interface TerminalDividerProps {
  label?: React.ReactNode
  className?: string
  dashed?: boolean
}

/**
 * Horizontal section divider rendered as `── LABEL ──`.
 * Used between iterations or as a section title with rule lines.
 */
export function TerminalDivider({
  label,
  className,
  dashed = false
}: TerminalDividerProps): React.ReactNode {
  return (
    <div
      className={cn(
        'flex items-center gap-3 py-2 font-mono text-[10px] uppercase tracking-[0.18em] text-muted-foreground-faint select-none',
        className
      )}
    >
      <span className={cn('flex-1', dashed ? 'tty-divider-dashed' : 'tty-divider')} />
      {label && <span className="whitespace-nowrap">— {label} —</span>}
      <span className={cn('flex-1', dashed ? 'tty-divider-dashed' : 'tty-divider')} />
    </div>
  )
}
