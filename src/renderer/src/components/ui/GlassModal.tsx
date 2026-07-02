import React, { useState, useEffect, useRef } from 'react'
import { createPortal } from 'react-dom'
import { motion, useReducedMotion } from 'framer-motion'
import { X } from 'lucide-react'
import { cn } from '../../lib/utils'
import { modalVariants, overlayVariants } from '../../lib/animations'

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'

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
  const reduceMotion = useReducedMotion()
  const containerRef = useRef<HTMLDivElement>(null)
  const previouslyFocused = useRef<HTMLElement | null>(null)

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent): void => {
      if (e.key === 'Escape' && isOpen) onClose()
    }
    document.addEventListener('keydown', handleEscape)
    return () => document.removeEventListener('keydown', handleEscape)
  }, [isOpen, onClose])

  useEffect(() => {
    if (!isOpen) {
      // Restore focus when closing
      previouslyFocused.current?.focus?.()
      previouslyFocused.current = null
      return
    }

    // Save the element that had focus before the modal opened
    previouslyFocused.current = document.activeElement as HTMLElement | null

    // Move focus into the modal
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

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <motion.div
        variants={overlayVariants}
        initial="hidden"
        animate="visible"
        className="absolute inset-0 bg-background/80 cursor-pointer"
        onClick={onClose}
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
        className={cn(
          'glass-modal relative w-full max-w-lg max-h-[90vh] flex flex-col font-mono outline-none',
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
      </motion.div>
    </div>,
    portalRoot
  )
}
