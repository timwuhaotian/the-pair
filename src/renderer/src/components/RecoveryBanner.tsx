import React from 'react'
import { useTranslation } from 'react-i18next'
import { AlertTriangle, X } from 'lucide-react'
import { type Pair } from '../store/usePairStore'
import { GlassButton } from './ui/GlassButton'
import { IconButton } from './ui/IconButton'

interface RecoveryBannerProps {
  pairs: Pair[]
  onOpenPair: (pair: Pair) => void
  onDismiss: () => void
}

/**
 * Non-blocking banner on the hub listing pairs that were restored in a
 * resumable state (paused, awaiting review, or errored). Crash recovery in
 * this app is snapshot rehydration at startup — there is no separate recovery
 * modal — so the banner points at the pairs that need a human decision.
 */
export function RecoveryBanner({
  pairs,
  onOpenPair,
  onDismiss
}: RecoveryBannerProps): React.ReactNode {
  const { t } = useTranslation()

  if (pairs.length === 0) return null

  return (
    <div
      role="status"
      data-testid="recovery-banner"
      className="shell-card flex shrink-0 items-start gap-2 border-state-running/50 bg-state-running/[0.07] px-3.5 py-2.5 text-[13px]"
    >
      <AlertTriangle size={13} aria-hidden className="mt-px shrink-0 state-running" />
      <div className="min-w-0 flex-1">
        <span className="font-semibold state-running">{t('recovery.title')}</span>{' '}
        <span className="text-foreground/85">
          {t('recovery.description', { count: pairs.length })}
        </span>
        <div className="mt-1.5 flex flex-wrap gap-1.5">
          {pairs.slice(0, 5).map((pair) => (
            <GlassButton
              key={pair.id}
              variant="secondary"
              size="xs"
              onClick={() => onOpenPair(pair)}
              data-testid={`recovery-open-${pair.id}`}
              className="max-w-[24ch] bg-background/60"
              title={pair.name}
            >
              <span className="min-w-0 truncate">{pair.name}</span>
            </GlassButton>
          ))}
        </div>
      </div>
      <IconButton
        onClick={onDismiss}
        aria-label={t('common.dismiss', { defaultValue: 'Dismiss' })}
        data-testid="recovery-dismiss"
      >
        <X size={12} />
      </IconButton>
    </div>
  )
}
