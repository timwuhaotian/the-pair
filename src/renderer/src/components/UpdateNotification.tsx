import React, { lazy, Suspense, useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { ArrowDownToLine, Loader2, X } from 'lucide-react'
import { motion, AnimatePresence } from 'framer-motion'
import { cn } from '../lib/utils'
import { useUpdateStore } from '../store/useUpdateStore'
import { overlayVariants, modalVariants } from '../lib/animations'

const MarkdownContent = lazy(() =>
  import('./MarkdownContent').then(({ MarkdownContent }) => ({
    default: MarkdownContent
  }))
)

export function UpdateNotification(): React.ReactNode {
  const [portalRoot] = useState<HTMLElement | null>(() => document.body)
  const phase = useUpdateStore((s) => s.phase)
  const version = useUpdateStore((s) => s.version)
  const progress = useUpdateStore((s) => s.progress)
  const message = useUpdateStore((s) => s.message)
  const releaseBody = useUpdateStore((s) => s.releaseBody)
  const showModal = useUpdateStore((s) => s.showModal)
  const showToast = useUpdateStore((s) => s.showToast)
  const toastMessage = useUpdateStore((s) => s.toastMessage)
  const toastType = useUpdateStore((s) => s.toastType)
  const setShowModal = useUpdateStore((s) => s.setShowModal)
  const clearToast = useUpdateStore((s) => s.clearToast)
  const reset = useUpdateStore((s) => s.reset)
  const installUpdate = useUpdateStore((s) => s.installUpdate)

  useEffect(() => {
    const timer = setTimeout(() => {
      clearToast()
    }, 4000)
    return () => clearTimeout(timer)
  }, [showToast, clearToast])

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent): void => {
      if (e.key === 'Escape' && showModal) {
        setShowModal(false)
      }
    }
    document.addEventListener('keydown', handleEscape)
    return () => document.removeEventListener('keydown', handleEscape)
  }, [showModal, setShowModal])

  if (!portalRoot) return null

  const handleCloseModal = (): void => {
    setShowModal(false)
    if (phase !== 'installing') {
      reset()
    }
  }

  const isInstalling = phase === 'installing'
  const installLabel =
    progress !== null
      ? `Installing ${progress}%`
      : isInstalling
        ? 'Installing...'
        : `Install v${version}`

  return createPortal(
    <>
      <AnimatePresence>
        {showModal && phase === 'available' && (
          <motion.div
            className="fixed inset-0 z-50 flex items-center justify-center p-4"
            variants={overlayVariants}
            initial="hidden"
            animate="visible"
            exit="hidden"
          >
            <div
              className="absolute inset-0 bg-black/60 backdrop-blur-sm cursor-pointer"
              onClick={handleCloseModal}
            />
            <motion.div
              className="glass-modal w-full max-w-xl shadow-2xl relative"
              variants={modalVariants}
              initial="hidden"
              animate="visible"
              exit="exit"
            >
              <div className="flex items-center justify-between px-6 pt-6 pb-4 border-b border-border">
                <h2 className="text-lg font-semibold text-foreground tracking-tight">
                  Update Available — v{version}
                </h2>
                <button
                  onClick={handleCloseModal}
                  disabled={isInstalling}
                  className={cn(
                    'p-1.5 rounded-lg text-muted-foreground hover:text-foreground hover:bg-muted/50 transition-all cursor-pointer',
                    isInstalling && 'opacity-50 cursor-not-allowed'
                  )}
                >
                  <X size={16} />
                </button>
              </div>
              <div className="p-6">
                {message && <p className="mb-4 text-sm text-muted-foreground">{message}</p>}
                {releaseBody && (
                  <div className="mb-6 max-h-[40vh] overflow-y-auto rounded-lg border border-border bg-muted/20 p-4 prose prose-sm dark:prose-invert max-w-none">
                    <Suspense fallback={null}>
                      <MarkdownContent content={releaseBody} />
                    </Suspense>
                  </div>
                )}
                <div className="flex items-center gap-3">
                  <button
                    onClick={() => void installUpdate()}
                    disabled={isInstalling}
                    className={cn(
                      'flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-all cursor-pointer',
                      'bg-primary text-primary-foreground hover:bg-primary/90',
                      isInstalling && 'opacity-70 cursor-not-allowed'
                    )}
                  >
                    {isInstalling ? (
                      <Loader2 size={14} className="animate-spin" />
                    ) : (
                      <ArrowDownToLine size={14} />
                    )}
                    {installLabel}
                  </button>
                  <button
                    onClick={handleCloseModal}
                    disabled={isInstalling}
                    className={cn(
                      'px-4 py-2 rounded-lg text-sm font-medium transition-all cursor-pointer',
                      'border border-border bg-background hover:bg-muted/50',
                      isInstalling && 'opacity-50 cursor-not-allowed'
                    )}
                  >
                    Remind me later
                  </button>
                </div>
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>

      <AnimatePresence>
        {showToast && toastMessage && (
          <motion.div
            initial={{ opacity: 0, y: -20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -20 }}
            className="fixed top-4 right-4 z-40"
          >
            <div
              className={cn(
                'px-4 py-3 rounded-lg shadow-lg text-sm font-medium',
                toastType === 'success' && 'bg-green-500/90 text-white',
                toastType === 'error' && 'bg-red-500/90 text-white',
                toastType === 'info' && 'bg-blue-500/90 text-white'
              )}
            >
              {toastMessage}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </>,
    portalRoot
  )
}
