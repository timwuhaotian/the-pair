import React from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'

interface IterationProgressProps {
  current: number
  max: number
  adaptiveBudget?: number
  className?: string
  /** Number of cells in the ASCII bar (default 20). */
  cells?: number
}

export function IterationProgress({
  current,
  max,
  adaptiveBudget,
  className,
  cells = 20
}: IterationProgressProps): React.ReactNode {
  const { t } = useTranslation()
  const effectiveMax = adaptiveBudget ?? max
  // A budget of 0 means unlimited: no proportional bar, no limit warnings.
  const isUnlimited = !effectiveMax || effectiveMax <= 0
  const filled = isUnlimited
    ? 0
    : Math.min(Math.round((current / Math.max(effectiveMax, 1)) * cells), cells)
  const empty = cells - filled
  const percentage = isUnlimited ? 0 : Math.min((current / Math.max(effectiveMax, 1)) * 100, 100)
  const isNearLimit = !isUnlimited && percentage >= 80
  const isAtLimit = !isUnlimited && current >= effectiveMax

  const stateClass = isAtLimit
    ? 'state-error'
    : isNearLimit
      ? 'state-running'
      : 'text-foreground/85'

  return (
    <div className={cn('space-y-1 font-mono text-[11px]', className)}>
      <div className="flex items-baseline justify-between">
        <span className="text-[10px] uppercase tracking-[0.16em] text-muted-foreground">
          {t(
            adaptiveBudget != null && adaptiveBudget !== max
              ? 'errors.iterationsAdaptive'
              : 'errors.iterations'
          )}
        </span>
        <span className={cn('tabular-nums', stateClass)}>
          {current}/{isUnlimited ? '∞' : effectiveMax}
        </span>
      </div>
      {isUnlimited ? (
        <div className="text-[10px] text-muted-foreground tabular-nums">[ ∞ unlimited ]</div>
      ) : (
        <div className={cn('flex items-baseline gap-0 tabular-nums', stateClass)}>
          <span aria-hidden>[</span>
          <span aria-hidden className="select-none">
            {'█'.repeat(filled)}
            <span className="text-muted-foreground/30">{'░'.repeat(empty)}</span>
          </span>
          <span aria-hidden>]</span>
        </div>
      )}
      {isAtLimit ? (
        <p className="text-[10px] state-error">{t('errors.iterationLimit')}</p>
      ) : isNearLimit ? (
        <div className="text-[10px] state-running space-y-0.5">
          <p className="font-bold uppercase tracking-[0.14em]">! {t('errors.approachingLimit')}</p>
          <p className="text-muted-foreground normal-case tracking-normal">
            {t('errors.approachingLimitHint')}
          </p>
        </div>
      ) : null}
    </div>
  )
}
