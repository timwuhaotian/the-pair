import React, { useState, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { X } from 'lucide-react'
import { cn } from '../../lib/utils'
import { motion, AnimatePresence } from 'framer-motion'
import { overlayVariants, modalVariants } from '../../lib/animations'

interface GlassModalProps {
  isOpen: boolean
  onClose: () => void
  children: React.ReactNode
  title?: string
  className?: string
}

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
      if (e.key === 'Escape' && isOpen) {
        onClose()
      }
    }
    document.addEventListener('keydown', handleEscape)
    return () => document.removeEventListener('keydown', handleEscape)
  }, [isOpen, onClose])

  if (!portalRoot) return null

  return createPortal(
    <AnimatePresence>
      {isOpen && (
        <motion.div
          className="fixed inset-0 z-50 flex items-center justify-center p-4"
          variants={overlayVariants}
          initial="hidden"
          animate="visible"
          exit="hidden"
        >
          <div
            className="absolute inset-0 bg-black/60 backdrop-blur-sm cursor-pointer"
            onClick={onClose}
          />
          <motion.div
            className={cn(
              'glass-modal w-full max-w-lg shadow-2xl relative max-h-[90vh] flex flex-col',
              className
            )}
            variants={modalVariants}
            initial="hidden"
            animate="visible"
            exit="exit"
          >
            {title && (
              <div className="flex items-center justify-between px-5 pt-4 pb-3 border-b border-border flex-shrink-0">
                <h2 className="text-base font-semibold text-foreground tracking-tight">{title}</h2>
                <button
                  onClick={onClose}
                  className="p-1.5 rounded-lg text-muted-foreground hover:text-foreground hover:bg-muted/50 transition-all cursor-pointer"
                >
                  <X size={15} />
                </button>
              </div>
            )}
            <div className="p-5 overflow-y-auto flex-1">{children}</div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    portalRoot
  )
}
