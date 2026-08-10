import React, { useState, useEffect, useRef } from 'react'
import { createPortal } from 'react-dom'
import { motion, useReducedMotion } from 'framer-motion'
import { AlertTriangle, X } from 'lucide-react'
import { cn } from '../../lib/utils'
import { GlassButton } from './GlassButton'
import { modalVariants, overlayVariants } from '../../lib/animations'

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'

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
  const reduceMotion = useReducedMotion()
  const containerRef = useRef<HTMLDivElement>(null)
  const previouslyFocused = useRef<HTMLElement | null>(null)

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent): void => {
      if (e.key === 'Escape' && isOpen) onCancel()
    }
    document.addEventListener('keydown', handleEscape)
    return () => document.removeEventListener('keydown', handleEscape)
  }, [isOpen, onCancel])

  useEffect(() => {
    if (!isOpen) {
      previouslyFocused.current?.focus?.()
      previouslyFocused.current = null
      return
    }

    previouslyFocused.current = document.activeElement as HTMLElement | null

    const container = containerRef.current
    if (container) {
      const focusable = container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)
      if (focusable.length > 0) {
        focusable[0].focus()
      } else {
        container.focus()
      }
    }

    const handleTab = (e: KeyboardEvent): void => {
      if (e.key !== 'Tab') return
      const el = containerRef.current
      if (!el) return
      const nodes = el.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)
      if (nodes.length === 0) {
        e.preventDefault()
        el.focus()
        return
      }
      const first = nodes[0]
      const last = nodes[nodes.length - 1]
      if (e.shiftKey) {
        if (document.activeElement === first || !el.contains(document.activeElement)) {
          e.preventDefault()
          last.focus()
        }
      } else {
        if (document.activeElement === last || !el.contains(document.activeElement)) {
          e.preventDefault()
          first.focus()
        }
      }
    }

    document.addEventListener('keydown', handleTab)
    return () => document.removeEventListener('keydown', handleTab)
  }, [isOpen])

  if (!portalRoot || !isOpen) return null

  const toneClass = variant === 'destructive' ? 'state-error' : 'state-running'

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 font-mono">
      <motion.div
        variants={overlayVariants}
        initial="hidden"
        animate="visible"
        className="absolute inset-0 bg-background/80 cursor-pointer"
        onClick={onCancel}
        aria-hidden
      />
      <motion.div
        ref={containerRef}
        variants={reduceMotion ? overlayVariants : modalVariants}
        initial="hidden"
        animate="visible"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        className="glass-modal relative w-full max-w-md outline-none"
      >
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
          <GlassButton
            variant="ghost"
            size="sm"
            onClick={onCancel}
            data-testid="confirm-modal-cancel"
          >
            {cancelLabel}
          </GlassButton>
          <GlassButton
            variant="destructive"
            size="sm"
            onClick={onConfirm}
            data-testid="confirm-modal-confirm"
          >
            {confirmLabel}
          </GlassButton>
        </div>
      </motion.div>
    </div>,
    portalRoot
  )
}
