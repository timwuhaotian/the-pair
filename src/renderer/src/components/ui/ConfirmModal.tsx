import React, { useState, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { AlertTriangle, X } from 'lucide-react'
import { cn } from '../../lib/utils'
import { GlassButton } from './GlassButton'

interface ConfirmModalProps {
  isOpen: boolean
  title: string
  message: string
  confirmLabel?: string
  cancelLabel?: string
  variant?: 'destructive' | 'warning'
  onConfirm: () => void
  onCancel: () => void
}

export function ConfirmModal({
  isOpen,
  title,
  message,
  confirmLabel = 'confirm',
  cancelLabel = 'cancel',
  variant = 'destructive',
  onConfirm,
  onCancel
}: ConfirmModalProps): React.ReactNode {
  const [portalRoot] = useState<HTMLElement | null>(() => document.body)

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent): void => {
      if (e.key === 'Escape' && isOpen) onCancel()
    }
    document.addEventListener('keydown', handleEscape)
    return () => document.removeEventListener('keydown', handleEscape)
  }, [isOpen, onCancel])

  if (!portalRoot || !isOpen) return null

  const toneClass = variant === 'destructive' ? 'state-error' : 'state-running'

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 font-mono">
      <div className="absolute inset-0 bg-background/80 cursor-pointer" onClick={onCancel} />
      <div className="glass-modal relative w-full max-w-md">
        <div className="flex items-baseline justify-between border-b border-border px-4 py-2.5">
          <div className="flex items-baseline gap-2 min-w-0">
            <span aria-hidden className="text-foreground/70 select-none">
              {'>_'}
            </span>
            <h2 className="text-[12px] uppercase tracking-[0.14em] font-bold text-foreground truncate">
              {title}
            </h2>
          </div>
          <button
            onClick={onCancel}
            aria-label="close"
            className="p-1 text-muted-foreground hover:text-foreground hover:bg-foreground/[0.06] transition-colors cursor-pointer rounded-sm"
          >
            <X size={13} />
          </button>
        </div>
        <div className="p-4">
          <div className="flex items-baseline gap-2">
            <AlertTriangle size={11} className={cn('translate-y-px', toneClass)} />
            <p className="text-[12px] leading-relaxed text-foreground/90">{message}</p>
          </div>
        </div>
        <div className="flex items-center justify-end gap-2 px-4 pb-3 pt-2 border-t border-border">
          <GlassButton variant="ghost" size="sm" onClick={onCancel}>
            {cancelLabel}
          </GlassButton>
          <GlassButton variant="destructive" size="sm" onClick={onConfirm}>
            {confirmLabel}
          </GlassButton>
        </div>
      </div>
    </div>,
    portalRoot
  )
}
