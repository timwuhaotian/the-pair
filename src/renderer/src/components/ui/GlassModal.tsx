import React, { useState, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { X } from 'lucide-react'
import { cn } from '../../lib/utils'

interface GlassModalProps {
  isOpen: boolean
  onClose: () => void
  children: React.ReactNode
  title?: string
  className?: string
}

/**
 * Terminal-style modal: square corners, hairline border, no blur, no shadow.
 * Backdrop is opaque ink to keep the "terminal stays in focus" feel without
 * the gauzy frosted-glass aesthetic.
 */
export function GlassModal({
  isOpen,
  onClose,
  children,
  title,
  className
}: GlassModalProps): React.ReactNode {
  const [portalRoot] = useState<HTMLElement | null>(() => document.body)

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent): void => {
      if (e.key === 'Escape' && isOpen) onClose()
    }
    document.addEventListener('keydown', handleEscape)
    return () => document.removeEventListener('keydown', handleEscape)
  }, [isOpen, onClose])

  if (!portalRoot || !isOpen) return null

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div
        className="absolute inset-0 bg-background/80 cursor-pointer"
        onClick={onClose}
        aria-hidden
      />
      <div
        className={cn(
          'glass-modal relative w-full max-w-lg max-h-[90vh] flex flex-col font-mono',
          className
        )}
      >
        {title && (
          <div className="flex items-baseline justify-between gap-2 border-b border-border px-4 py-2.5 shrink-0">
            <div className="flex items-baseline gap-2 min-w-0">
              <span aria-hidden className="text-foreground/70 select-none">
                {'>_'}
              </span>
              <h2 className="truncate text-[12px] font-bold uppercase tracking-[0.14em] text-foreground">
                {title}
              </h2>
            </div>
            <button
              onClick={onClose}
              aria-label="close"
              className="p-1 text-muted-foreground hover:text-foreground hover:bg-foreground/[0.06] transition-colors cursor-pointer rounded-sm"
            >
              <X size={13} />
            </button>
          </div>
        )}
        <div className="p-5 overflow-y-auto flex-1 scrollbar-thin">{children}</div>
      </div>
    </div>,
    portalRoot
  )
}
