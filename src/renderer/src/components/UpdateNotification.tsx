import React, { lazy, Suspense, useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { ArrowDownToLine, Loader2 } from 'lucide-react'
import { cn } from '../lib/utils'
import { Z } from '../lib/ui'
import { useUpdateStore } from '../store/useUpdateStore'
import { GlassButton } from './ui/GlassButton'
import { GlassModal } from './ui/GlassModal'
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

  const handleCloseModal = (): void => {
    if (!canDismiss) return
    setShowModal(false)
    reset()
  }

  const isInstalling = phase === 'installing'
  const isError = phase === 'error'
  const installLabel =
    isInstalling && progress !== null
      ? t('updates.installingPercent', { percent: progress })
      : isInstalling
        ? t('updates.installing')
        : t('updates.install', { version: version ?? '' })

  const toastTone =
    toastType === 'success'
      ? 'border-state-done bg-state-done state-done'
      : toastType === 'error'
        ? 'border-state-error bg-state-error state-error'
        : 'border-role-mentor bg-role-mentor role-mentor'

  const toastGlyph = toastType === 'success' ? '✓' : toastType === 'error' ? '✗' : '·'

  return (
    <>
      <GlassModal
        isOpen={isModalVisible}
        onClose={handleCloseModal}
        dismissible={canDismiss}
        title={
          isError
            ? t('updates.failedTitle')
            : version
              ? t('updates.available', { version })
              : t('updates.installing')
        }
        className="max-w-xl"
        data-testid="update-modal"
      >
        {message && (
          <p
            role={isError ? 'alert' : undefined}
            className={cn(
              'mb-3 text-[12px] [overflow-wrap:anywhere]',
              isError ? 'state-error' : 'text-muted-foreground'
            )}
          >
            {message}
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
          <div className="max-h-[40vh] overflow-y-auto scrollbar-thin rounded-md border border-border bg-background/40 p-3 text-[12px] leading-relaxed">
            <Suspense fallback={null}>
              <MarkdownContent content={releaseBody} />
            </Suspense>
          </div>
        )}
        <div className="mt-3 flex justify-end gap-2 border-t border-border pt-3">
          {isError ? (
            <GlassButton
              variant="secondary"
              onClick={handleCloseModal}
              data-testid="update-error-close"
            >
              {t('updates.close')}
            </GlassButton>
          ) : (
            <>
              <GlassButton variant="secondary" onClick={handleCloseModal} disabled={isInstalling}>
                {t('updates.remindLater')}
              </GlassButton>
              <GlassButton
                variant="primary"
                onClick={() => void installUpdate()}
                disabled={isInstalling}
                icon={
                  isInstalling ? (
                    <Loader2 size={12} className="animate-spin" />
                  ) : (
                    <ArrowDownToLine size={12} />
                  )
                }
              >
                {installLabel}
              </GlassButton>
            </>
          )}
        </div>
      </GlassModal>

      {portalRoot &&
        showToast &&
        toastMessage &&
        createPortal(
          <div className={cn('fixed top-3 right-3 font-mono', Z.toast)}>
            <div
              className={cn(
                'flex items-baseline gap-2 rounded-sm border px-3 py-2 text-[11px]',
                toastTone
              )}
            >
              <span aria-hidden className="select-none">
                {toastGlyph}
              </span>
              <span>{toastMessage}</span>
            </div>
          </div>,
          portalRoot
        )}
    </>
  )
}
