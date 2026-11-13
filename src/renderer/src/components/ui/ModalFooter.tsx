import React from 'react'
import { cn } from '../../lib/utils'

interface ModalFooterProps {
  children: React.ReactNode
  /** Optional leading control (e.g. a rescan button); actions stay right-aligned. */
  start?: React.ReactNode
  className?: string
}

/**
 * In-flow dialog footer: sits below the scroll region, never sticky, so long
 * content scrolls clear of it. Actions are right-aligned.
 */
export function ModalFooter({ children, start, className }: ModalFooterProps): React.ReactNode {
  return (
    <div
      className={cn(
        'mt-3 flex shrink-0 items-center gap-2 border-t border-border pt-3',
        start ? 'justify-between' : 'justify-end',
        className
      )}
    >
      {start ? (
        <>
          {start}
          <div className="flex items-center gap-2">{children}</div>
        </>
      ) : (
        children
      )}
    </div>
  )
}
