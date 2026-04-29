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
      <ArrowDownToLine size={13} className="transition-transform group-hover:translate-y-0.5" />
    ) : phase === 'checking' ? (
      <Loader2 size={13} className="animate-spin" />
    ) : phase === 'installing' ? (
      <Loader2 size={13} className="animate-spin" />
    ) : isUpToDate ? (
      <CheckCircle2 size={13} />
    ) : isError ? (
      <XCircle size={13} />
    ) : (
      <RefreshCw size={13} className="transition-transform group-hover:rotate-45" />
    )

  const variant = phase === 'available' ? 'primary' : 'secondary'

  return (
    <div className="flex items-center gap-2">
      <GlassButton
        variant={variant}
        size="sm"
        onClick={phase === 'available' ? handleInstall : handleCheckUpdates}
        disabled={isBusy}
        icon={icon}
        className={cn(
          'group whitespace-nowrap transition-all duration-300',
          isError &&
            'border-red-500/40 bg-red-500/5 text-red-600 dark:text-red-400 hover:bg-red-500/10 hover:border-red-500/60',
          isUpToDate &&
            'border-green-500/40 bg-green-500/5 text-green-600 dark:text-green-400 hover:bg-green-500/10 hover:border-green-500/60'
        )}
        title={message || undefined}
      >
        {label}
      </GlassButton>
      {phase === 'available' && version && releaseBody && (
        <button
          onClick={handleShowReleaseNotes}
          className="p-1.5 rounded-lg text-muted-foreground hover:text-foreground hover:bg-muted/50 transition-all cursor-pointer"
          title="View release notes"
        >
          <Info size={14} />
        </button>
      )}
    </div>
  )
}
