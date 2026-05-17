import React, { lazy, Suspense, useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { ArrowDownToLine, Loader2, X } from 'lucide-react'
import { cn } from '../lib/utils'
import { useUpdateStore } from '../store/useUpdateStore'

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
    const timer = setTimeout(() => clearToast(), 4000)
    return () => clearTimeout(timer)
  }, [showToast, clearToast])

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent): void => {
      if (e.key === 'Escape' && showModal) setShowModal(false)
    }
    document.addEventListener('keydown', handleEscape)
    return () => document.removeEventListener('keydown', handleEscape)
  }, [showModal, setShowModal])

  if (!portalRoot) return null

  const handleCloseModal = (): void => {
    setShowModal(false)
    if (phase !== 'installing') reset()
  }

  const isInstalling = phase === 'installing'
  const installLabel =
    progress !== null
      ? `installing ${progress}%`
      : isInstalling
        ? 'installing…'
        : `install v${version}`

  const toastTone =
    toastType === 'success'
      ? 'border-state-done bg-state-done state-done'
      : toastType === 'error'
        ? 'border-state-error bg-state-error state-error'
        : 'border-role-mentor bg-role-mentor role-mentor'

  const toastGlyph = toastType === 'success' ? '✓' : toastType === 'error' ? '✗' : '·'

  return createPortal(
    <>
      {showModal && phase === 'available' && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 font-mono">
          <div
            className="absolute inset-0 bg-background/80 cursor-pointer"
            onClick={handleCloseModal}
            aria-hidden
          />
          <div className="glass-modal relative w-full max-w-xl">
            <div className="flex items-baseline justify-between gap-2 border-b border-border px-4 py-2.5">
              <div className="flex items-baseline gap-2 min-w-0">
                <span aria-hidden className="text-foreground/70 select-none">
                  {'>_'}
                </span>
                <h2 className="text-[12px] font-bold uppercase tracking-[0.14em] text-foreground">
                  update available — v{version}
                </h2>
              </div>
              <button
                onClick={handleCloseModal}
                disabled={isInstalling}
                className={cn(
                  'p-1 rounded-sm text-muted-foreground hover:text-foreground hover:bg-foreground/[0.06] transition-colors cursor-pointer',
                  isInstalling && 'opacity-40 cursor-not-allowed'
                )}
                aria-label="close"
              >
                <X size={13} />
              </button>
            </div>
            <div className="p-4">
              {message && <p className="mb-3 text-[11px] text-muted-foreground">· {message}</p>}
              {releaseBody && (
                <div className="mb-4 max-h-[40vh] overflow-y-auto scrollbar-thin border border-border bg-background/40 p-3 text-[12px] leading-relaxed">
                  <Suspense fallback={null}>
                    <MarkdownContent content={releaseBody} />
                  </Suspense>
                </div>
              )}
              <div className="flex items-center gap-2">
                <button
                  onClick={() => void installUpdate()}
                  disabled={isInstalling}
                  className={cn(
                    'inline-flex items-center gap-2 border border-foreground bg-foreground text-background px-3 py-1.5 text-[12px] font-bold uppercase tracking-[0.12em] rounded-sm cursor-pointer hover:bg-foreground/90 transition-colors',
                    isInstalling && 'opacity-60 cursor-not-allowed'
                  )}
                >
                  {isInstalling ? (
                    <Loader2 size={11} className="animate-spin" />
                  ) : (
                    <ArrowDownToLine size={11} />
                  )}
                  ▸ {installLabel}
                </button>
                <button
                  onClick={handleCloseModal}
                  disabled={isInstalling}
                  className={cn(
                    'inline-flex items-center px-3 py-1.5 text-[12px] uppercase tracking-[0.12em] rounded-sm cursor-pointer border border-border bg-transparent text-foreground/85 hover:bg-foreground/[0.06] hover:border-foreground/40 transition-colors',
                    isInstalling && 'opacity-40 cursor-not-allowed'
                  )}
                >
                  remind me later
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {showToast && toastMessage && (
        <div className="fixed top-3 right-3 z-40 font-mono">
          <div
            className={cn(
              'flex items-baseline gap-2 border px-3 py-2 text-[11px] rounded-sm',
              toastTone
            )}
          >
            <span aria-hidden className="select-none">
              {toastGlyph}
            </span>
            <span>{toastMessage}</span>
          </div>
        </div>
      )}
    </>,
    portalRoot
  )
}
