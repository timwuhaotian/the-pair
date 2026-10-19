import React, { useState, useEffect, useRef } from 'react'
import { createPortal } from 'react-dom'
import { motion, useReducedMotion } from 'framer-motion'
import { X } from 'lucide-react'
import { cn } from '../../lib/utils'
import { Z } from '../../lib/ui'
import { modalVariants, overlayVariants } from '../../lib/animations'
import { shouldCloseOnEscape } from '../keyboard'
import { IconButton } from './IconButton'

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'

interface GlassModalProps {
  isOpen: boolean
  onClose: () => void
  children: React.ReactNode
  title?: string
  className?: string
  /** False while the dialog must stay up (e.g. mid-install): Esc, the backdrop
   *  and the close button are inert. */
  dismissible?: boolean
  'data-testid'?: string
}

/**
 * Shell modal: card surface, medium radius, focus trap + Esc handling.
 * The backdrop stays a flat dim so the conversation underneath stays readable.
 */
export function GlassModal({
  isOpen,
  onClose,
  children,
  title,
  className,
  dismissible = true,
  'data-testid': dataTestId
}: GlassModalProps): React.ReactNode {
  const [portalRoot] = useState<HTMLElement | null>(() => document.body)
  const reduceMotion = useReducedMotion()
  const containerRef = useRef<HTMLDivElement>(null)
  const previouslyFocused = useRef<HTMLElement | null>(null)

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent): void => {
      // Ignore Escape already handled by a nested dropdown/popover (defaultPrevented)
      // or used to cancel an IME composition — closing would discard the draft.
      if (isOpen && dismissible && shouldCloseOnEscape(e)) onClose()
    }
    document.addEventListener('keydown', handleEscape)
    return () => document.removeEventListener('keydown', handleEscape)
  }, [isOpen, dismissible, onClose])

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
    <div className={cn('fixed inset-0 flex items-center justify-center p-4', Z.modal)}>
      <motion.div
        variants={overlayVariants}
        initial="hidden"
        animate="visible"
        className={cn(
          'absolute inset-0 bg-background/80',
          dismissible ? 'cursor-pointer' : 'cursor-wait'
        )}
        onClick={dismissible ? onClose : undefined}
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
        data-testid={dataTestId}
        className={cn(
          'shell-card relative flex max-h-[90vh] w-full max-w-lg flex-col outline-none',
          className
        )}
      >
        {title && (
          <div className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-4 py-3">
            <h2 className="min-w-0 truncate text-[14px] font-semibold tracking-tight text-foreground">
              {title}
            </h2>
            <IconButton aria-label="close" onClick={onClose} disabled={!dismissible}>
              <X size={14} />
            </IconButton>
          </div>
        )}
        <div className="p-5 overflow-y-auto flex-1 scrollbar-thin">{children}</div>
      </motion.div>
    </div>,
    portalRoot
  )
}
