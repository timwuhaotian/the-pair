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
  /**
   * Accessible name of the dialog. Required: it names the dialog and keeps
   * the close button in the header bar.
   */
  title: string
  /**
   * Custom header content. Replaces the default title block inside the
   * dialog's header bar (the close button stays). It must be shrinkable and
   * truncate its own text — the bar gives it a bounded box.
   */
  header?: React.ReactNode
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
  header,
  className,
  dismissible = true,
  'data-testid': dataTestId
}: GlassModalProps): React.ReactNode {
  const [portalRoot] = useState<HTMLElement | null>(() => document.body)
  const reduceMotion = useReducedMotion()
  const containerRef = useRef<HTMLDivElement>(null)
  const bodyRef = useRef<HTMLDivElement>(null)
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

    // Move focus into the modal — into its body, not the header bar: the
    // close button is chrome, the content is what the user came for.
    const container = containerRef.current
    if (container) {
      const focusable = (bodyRef.current ?? container).querySelectorAll<HTMLElement>(
        FOCUSABLE_SELECTOR
      )
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
      // Focus on the dialog itself counts as "outside" the focusables: the
      // next Tab has to land on a control, never escape the dialog.
      const outside = !el.contains(document.activeElement) || document.activeElement === el
      if (e.shiftKey) {
        if (document.activeElement === first || outside) {
          e.preventDefault()
          last.focus()
        }
      } else {
        if (document.activeElement === last || outside) {
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
        <div className="flex shrink-0 items-center justify-between gap-3 border-b border-border px-4 py-3">
          {header ? (
            // Bounded box: a custom header must not push the close button
            // out of the card.
            <div className="min-w-0 flex-1 overflow-hidden">{header}</div>
          ) : (
            <h2 className="min-w-0 truncate text-[14px] font-semibold tracking-tight text-foreground">
              {title}
            </h2>
          )}
          <IconButton aria-label="close" onClick={onClose} disabled={!dismissible}>
            <X size={14} />
          </IconButton>
        </div>
        {/* min-h-0 lets the body shrink inside the max-h card, so long
            content scrolls inside the dialog instead of pushing the footer
            (and the submit button) out of it. */}
        <div ref={bodyRef} className="min-h-0 flex-1 overflow-y-auto p-5 scrollbar-thin">
          {children}
        </div>
      </motion.div>
    </div>,
    portalRoot
  )
}
