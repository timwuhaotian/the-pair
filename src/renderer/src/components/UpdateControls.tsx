import { emit } from '@tauri-apps/api/event'
import { useTranslation } from 'react-i18next'
import { ArrowDownToLine, CheckCircle2, Info, Loader2, RefreshCw, XCircle } from 'lucide-react'
import { useUpdateStore } from '../store/useUpdateStore'
import { GlassButton } from './ui/GlassButton'
import { cn } from '../lib/utils'

export function UpdateControls(): React.ReactNode {
  const { t } = useTranslation()
  const phase = useUpdateStore((s) => s.phase)
  const version = useUpdateStore((s) => s.version)
  const progress = useUpdateStore((s) => s.progress)
  const message = useUpdateStore((s) => s.message)
  const releaseBody = useUpdateStore((s) => s.releaseBody)
  const setShowModal = useUpdateStore((s) => s.setShowModal)
  const installUpdate = useUpdateStore((s) => s.installUpdate)

  const handleCheckUpdates = (): void => {
    void emit('app:update:check')
  }

  const handleInstall = (): void => {
    void installUpdate()
  }

  const handleShowReleaseNotes = (): void => {
    setShowModal(true)
  }

  const isBusy = phase === 'checking' || phase === 'installing'
  const isUpToDate = phase === 'up-to-date'
  const isError = phase === 'error'

  const label =
    phase === 'available' && version
      ? t('updates.install', { version })
      : phase === 'checking'
        ? t('updates.checking')
        : phase === 'installing'
          ? progress !== null
            ? t('updates.installingPercent', { percent: progress })
            : t('updates.installing')
          : isUpToDate
            ? t('updates.upToDate')
            : isError
              ? t('updates.checkAgain')
              : t('updates.checkUpdates')

  const icon =
    phase === 'available' ? (
      <ArrowDownToLine size={11} />
    ) : phase === 'checking' || phase === 'installing' ? (
      <Loader2 size={11} className="animate-spin" />
    ) : isUpToDate ? (
      <CheckCircle2 size={11} />
    ) : isError ? (
      <XCircle size={11} />
    ) : (
      <RefreshCw size={11} />
    )

  const variant = phase === 'available' ? 'primary' : 'secondary'

  return (
    <div className="flex items-center gap-1.5">
      <GlassButton
        variant={variant}
        size="sm"
        onClick={phase === 'available' ? handleInstall : handleCheckUpdates}
        disabled={isBusy}
        icon={icon}
        className={cn(
          'whitespace-nowrap',
          isError && 'border-state-error state-error hover:bg-state-error',
          isUpToDate && 'border-state-done state-done hover:bg-state-done'
        )}
        title={message || undefined}
      >
        {label}
      </GlassButton>
      {phase === 'available' && version && releaseBody && (
        <button
          onClick={handleShowReleaseNotes}
          className="p-1 rounded-sm text-muted-foreground hover:text-foreground hover:bg-foreground/[0.06] transition-colors cursor-pointer"
          title="release notes"
        >
          <Info size={12} />
        </button>
      )}
    </div>
  )
}
