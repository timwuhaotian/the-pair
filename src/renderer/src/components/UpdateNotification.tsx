import React, { lazy, Suspense, useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { ArrowDownToLine, CheckCircle2, Loader2, RotateCw, XCircle } from 'lucide-react'
import type { TFunction } from 'i18next'
import { cn } from '../lib/utils'
import { Z } from '../lib/ui'
import { useUpdateStore } from '../store/useUpdateStore'
import { GlassButton } from './ui/GlassButton'
import { GlassModal } from './ui/GlassModal'
import {
  canDismissUpdateModal,
  formatMegabytes,
  keepsStateOnDismiss,
  shouldShowUpdateModal
} from './updateView'

const MarkdownContent = lazy(() =>
  import('./MarkdownContent').then(({ MarkdownContent }) => ({
    default: MarkdownContent
  }))
)

type Phase = ReturnType<typeof useUpdateStore.getState>['phase']

interface PhaseView {
  title: string
  body: string
  icon: React.ReactNode
  tone: 'busy' | 'done' | 'error'
}

function describePhase(
  phase: Phase,
  version: string,
  isUnpacking: boolean,
  t: TFunction
): PhaseView {
  const spinner = <Loader2 size={14} className="animate-spin" />
  const done = <CheckCircle2 size={14} />
  switch (phase) {
    case 'checking':
      return {
        title: t('updates.checkingTitle'),
        body: t('updates.checkingBody'),
        icon: spinner,
        tone: 'busy'
      }
    case 'up-to-date':
      return {
        title: t('updates.upToDateTitle'),
        body: t('updates.upToDateBody'),
        icon: done,
        tone: 'done'
      }
    case 'available':
      return {
        title: t('updates.available', { version }),
        body: '',
        icon: <ArrowDownToLine size={14} />,
        tone: 'busy'
      }
    case 'installing':
      return {
        title: t('updates.updatingTitle', { version }),
        body: isUnpacking ? t('updates.installingBody') : t('updates.downloadingBody'),
        icon: spinner,
        tone: 'busy'
      }
    case 'ready':
      return {
        title: t('updates.readyTitle', { version }),
        body: t('updates.readyBody'),
        icon: done,
        tone: 'done'
      }
    default:
      return {
        title: t('updates.failedTitle'),
        body: '',
        icon: <XCircle size={14} />,
        tone: 'error'
      }
  }
}

const STEPS = ['download', 'install', 'restart'] as const

/** Bar + caption + Download → Install → Restart steps for checking/installing/ready. */
function UpdateProgress({
  phase,
  progress,
  downloadedBytes,
  totalBytes
}: {
  phase: Phase
  progress: number | null
  downloadedBytes: number
  totalBytes: number | null
}): React.ReactNode {
  const { t } = useTranslation()
  const isReady = phase === 'ready'
  const isUnpacking = phase === 'installing' && progress === 100
  // Checking, an unknown download size and unpacking have no measurable progress.
  const determinate = isReady || (phase === 'installing' && progress !== null && !isUnpacking)
  const percent = isReady ? 100 : (progress ?? 0)
  const current = isReady ? 2 : isUnpacking ? 1 : 0

  const caption = isReady
    ? t('updates.stepDone')
    : isUnpacking
      ? t('updates.installing')
      : phase === 'checking'
        ? t('updates.checking')
        : progress !== null
          ? t('updates.downloadingPercent', { percent })
          : t('updates.downloading')
  const bytes =
    phase === 'installing' && !isUnpacking && downloadedBytes > 0
      ? totalBytes
        ? `${formatMegabytes(downloadedBytes)} / ${formatMegabytes(totalBytes)}`
        : formatMegabytes(downloadedBytes)
      : null

  return (
    <div className="mb-1" data-testid="update-progress">
      <div
        role="progressbar"
        aria-label={caption}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={determinate ? percent : undefined}
        className="relative h-1.5 w-full overflow-hidden rounded-full bg-foreground/[0.08]"
      >
        {determinate ? (
          <div
            className={cn(
              'h-full rounded-full transition-[width] duration-300 ease-out',
              isReady ? 'bg-[var(--state-done)]' : 'bg-foreground'
            )}
            style={{ width: `${percent}%` }}
          />
        ) : (
          <div className="update-progress-indeterminate absolute inset-y-0 w-1/3 rounded-full bg-foreground/60" />
        )}
      </div>
      <div className="mt-1.5 flex justify-between font-mono text-[11px] text-muted-foreground tabular-nums">
        <span className={cn(isReady && 'state-done')}>{caption}</span>
        {bytes && <span>{bytes}</span>}
      </div>
      {phase !== 'checking' && (
        <ol className="mt-3 flex items-center gap-2 text-[11px]">
          {STEPS.map((step, index) => {
            const state = index < current ? 'done' : index === current ? 'active' : 'todo'
            return (
              <li key={step} className="flex flex-1 items-center gap-2">
                <span
                  className={cn(
                    'flex items-center gap-1.5 whitespace-nowrap',
                    state === 'done' && 'state-done',
                    state === 'active' && 'text-foreground',
                    state === 'todo' && 'text-muted-foreground/60'
                  )}
                >
                  {state === 'done' ? (
                    <CheckCircle2 size={11} aria-hidden />
                  ) : state === 'active' && isReady ? (
                    <RotateCw size={11} aria-hidden />
                  ) : state === 'active' ? (
                    <Loader2 size={11} className="animate-spin" aria-hidden />
                  ) : (
                    <span aria-hidden className="size-[11px] rounded-full border border-current" />
                  )}
                  {t(`updates.step.${step}`)}
                </span>
                {index < STEPS.length - 1 && <span aria-hidden className="h-px flex-1 bg-border" />}
              </li>
            )
          })}
        </ol>
      )}
    </div>
  )
}

export function UpdateNotification(): React.ReactNode {
  const { t } = useTranslation()
  const [portalRoot] = useState<HTMLElement | null>(() => document.body)
  const phase = useUpdateStore((s) => s.phase)
  const version = useUpdateStore((s) => s.version)
  const progress = useUpdateStore((s) => s.progress)
  const downloadedBytes = useUpdateStore((s) => s.downloadedBytes)
  const totalBytes = useUpdateStore((s) => s.totalBytes)
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
  const restartToUpdate = useUpdateStore((s) => s.restartToUpdate)

  useEffect(() => {
    const timer = setTimeout(() => clearToast(), 4000)
    return () => clearTimeout(timer)
  }, [showToast, clearToast])

  const isModalVisible = shouldShowUpdateModal(showModal, phase)
  const canDismiss = canDismissUpdateModal(phase)

  const handleCloseModal = (): void => {
    if (!canDismiss) return
    setShowModal(false)
    if (!keepsStateOnDismiss(phase)) reset()
  }

  const isInstalling = phase === 'installing'
  const isError = phase === 'error'
  // Download is done once progress hits 100; the bundle is then swapped in place.
  const isUnpacking = isInstalling && progress === 100
  const view = describePhase(phase, version ?? '', isUnpacking, t)

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
        title={view.title}
        className="max-w-xl"
        data-testid="update-modal"
      >
        <div className="mb-3 flex items-start gap-3" data-testid={`update-phase-${phase}`}>
          <span
            aria-hidden
            className={cn(
              'mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-full border',
              view.tone === 'done' && 'border-state-done state-done',
              view.tone === 'error' && 'border-state-error state-error',
              view.tone === 'busy' && 'border-border text-foreground'
            )}
          >
            {view.icon}
          </span>
          <p
            role={isError ? 'alert' : 'status'}
            aria-live="polite"
            className={cn(
              'pt-1 text-[12px] [overflow-wrap:anywhere]',
              isError ? 'state-error' : 'text-muted-foreground'
            )}
          >
            {isError || phase === 'available' ? message : view.body}
          </p>
        </div>
        {(phase === 'checking' || isInstalling || phase === 'ready') && (
          <UpdateProgress
            phase={phase}
            progress={progress}
            downloadedBytes={downloadedBytes}
            totalBytes={totalBytes}
          />
        )}
        {phase === 'available' && releaseBody && (
          <div className="max-h-[40vh] overflow-y-auto scrollbar-thin rounded-md border border-border bg-background/40 p-3 text-[12px] leading-relaxed">
            <Suspense fallback={null}>
              <MarkdownContent content={releaseBody} />
            </Suspense>
          </div>
        )}
        {!isInstalling && (
          <div className="mt-3 flex justify-end gap-2 border-t border-border pt-3">
            {phase === 'available' ? (
              <>
                <GlassButton variant="secondary" onClick={handleCloseModal}>
                  {t('updates.remindLater')}
                </GlassButton>
                <GlassButton
                  variant="primary"
                  onClick={() => void installUpdate()}
                  icon={<ArrowDownToLine size={12} />}
                >
                  {t('updates.install', { version: version ?? '' })}
                </GlassButton>
              </>
            ) : phase === 'ready' ? (
              <>
                <GlassButton
                  variant="secondary"
                  onClick={handleCloseModal}
                  data-testid="update-restart-later"
                >
                  {t('updates.later')}
                </GlassButton>
                <GlassButton
                  variant="primary"
                  onClick={() => void restartToUpdate()}
                  icon={<RotateCw size={12} />}
                  data-testid="update-restart-now"
                >
                  {t('updates.restartNow')}
                </GlassButton>
              </>
            ) : (
              <GlassButton
                variant="secondary"
                onClick={handleCloseModal}
                data-testid={isError ? 'update-error-close' : 'update-close'}
              >
                {t('updates.close')}
              </GlassButton>
            )}
          </div>
        )}
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
