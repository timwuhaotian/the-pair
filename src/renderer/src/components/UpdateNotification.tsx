import React, { lazy, Suspense, useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { ArrowDownToLine, Loader2, X } from 'lucide-react'
import { cn } from '../lib/utils'
import { useUpdateStore } from '../store/useUpdateStore'
import { shouldCloseOnEscape } from './keyboard'
import { canDismissUpdateModal, shouldShowUpdateModal } from './updateView'

const MarkdownContent = lazy(() =>
  import('./MarkdownContent').then(({ MarkdownContent }) => ({
    default: MarkdownContent
  }))
)

export function UpdateNotification(): React.ReactNode {
  const { t } = useTranslation()
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

  const isModalVisible = shouldShowUpdateModal(showModal, phase)
  const canDismiss = canDismissUpdateModal(phase)

  useEffect(() => {
    if (!isModalVisible) return
    const handleEscape = (e: KeyboardEvent): void => {
      // Escape must not hide the modal mid-install — progress would vanish.
      if (!shouldCloseOnEscape(e) || !canDismiss) return
      setShowModal(false)
      reset()
    }
    document.addEventListener('keydown', handleEscape)
    return () => document.removeEventListener('keydown', handleEscape)
  }, [isModalVisible, canDismiss, setShowModal, reset])

  if (!portalRoot) return null

  const handleCloseModal = (): void => {
    if (!canDismiss) return
    setShowModal(false)
    reset()
  }

  const isInstalling = phase === 'installing'
  const isError = phase === 'error'
  const installLabel =
    isInstalling && progress !== null
      ? t('updates.installingPercent', { percent: progress }).toLowerCase()
      : isInstalling
        ? t('updates.installing').toLowerCase()
        : t('updates.install', { version: version ?? '' }).toLowerCase()

  const toastTone =
    toastType === 'success'
      ? 'border-state-done bg-state-done state-done'
      : toastType === 'error'
        ? 'border-state-error bg-state-error state-error'
        : 'border-role-mentor bg-role-mentor role-mentor'

  const toastGlyph = toastType === 'success' ? '✓' : toastType === 'error' ? '✗' : '·'

  return createPortal(
    <>
      {isModalVisible && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 font-mono">
          <div
            className={cn(
              'absolute inset-0 bg-background/80',
              canDismiss ? 'cursor-pointer' : 'cursor-wait'
            )}
            onClick={handleCloseModal}
            aria-hidden
          />
          <div
            className="glass-modal relative w-full max-w-xl"
            role="dialog"
            aria-modal="true"
            data-testid="update-modal"
          >
            <div className="flex items-baseline justify-between gap-2 border-b border-border px-4 py-2.5">
              <div className="flex items-baseline gap-2 min-w-0">
                <span aria-hidden className="text-foreground/70 select-none">
                  {'>_'}
                </span>
                <h2 className="text-[12px] font-bold uppercase tracking-[0.14em] text-foreground">
                  {isError
                    ? t('updates.failedTitle')
                    : version
                      ? t('updates.available', { version })
                      : t('updates.installing')}
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
              {message && (
                <p
                  role={isError ? 'alert' : undefined}
                  className={cn(
                    'mb-3 text-[11px] [overflow-wrap:anywhere]',
                    isError ? 'state-error' : 'text-muted-foreground'
                  )}
                >
                  {isError ? '✗' : '·'} {message}
                </p>
              )}
              {isInstalling && (
                <div className="mb-3" data-testid="update-progress">
                  <div className="h-1 w-full overflow-hidden rounded-sm bg-foreground/[0.08]">
                    {progress !== null ? (
                      <progress
                        value={progress}
                        max={100}
                        aria-label={installLabel}
                        className="block h-1 w-full appearance-none [&::-webkit-progress-bar]:bg-transparent [&::-webkit-progress-value]:bg-foreground"
                      />
                    ) : (
                      <div className="h-1 w-1/3 animate-pulse bg-foreground/60" />
                    )}
                  </div>
                </div>
              )}
              {!isError && releaseBody && (
                <div className="mb-4 max-h-[40vh] overflow-y-auto scrollbar-thin border border-border bg-background/40 p-3 text-[12px] leading-relaxed">
                  <Suspense fallback={null}>
                    <MarkdownContent content={releaseBody} />
                  </Suspense>
                </div>
              )}
              <div className="flex items-center gap-2">
                {isError ? (
                  <button
                    onClick={handleCloseModal}
                    data-testid="update-error-close"
                    className="inline-flex items-center px-3 py-1.5 text-[12px] uppercase tracking-[0.12em] rounded-sm cursor-pointer border border-border bg-transparent text-foreground/85 hover:bg-foreground/[0.06] hover:border-foreground/40 transition-colors"
                  >
                    {t('updates.close')}
                  </button>
                ) : (
                  <>
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
                      {t('updates.remindLater').toLowerCase()}
                    </button>
                  </>
                )}
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
