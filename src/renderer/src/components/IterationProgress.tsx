import React from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'

interface IterationProgressProps {
  current: number
  max: number
  adaptiveBudget?: number
  className?: string
}

export function IterationProgress({
  current,
  max,
  adaptiveBudget,
  className
}: IterationProgressProps): React.ReactNode {
  const { t } = useTranslation()
  const effectiveMax = adaptiveBudget ?? max
  const percentage = Math.min((current / effectiveMax) * 100, 100)
  const isNearLimit = percentage >= 80
  const isAtLimit = current >= effectiveMax

  return (
    <div className={cn('space-y-1.5', className)}>
      <div className="flex items-center justify-between text-[10px]">
        <span className="font-medium text-muted-foreground">
          {t(
            adaptiveBudget != null && adaptiveBudget !== max
              ? 'errors.iterationsAdaptive'
              : 'errors.iterations'
          )}
        </span>
        <span
          className={cn(
            'font-mono font-semibold',
            isAtLimit
              ? 'text-red-600 dark:text-red-400'
              : isNearLimit
                ? 'text-amber-600 dark:text-amber-400'
                : 'text-foreground/70'
          )}
        >
          {current}/{effectiveMax}
        </span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-muted">
        <div
          className={cn(
            'h-full rounded-full transition-all duration-500',
            isAtLimit ? 'bg-red-500' : isNearLimit ? 'bg-amber-500' : 'bg-primary'
          )}
          style={{ width: `${percentage}%` }}
        />
      </div>
      {isAtLimit && (
        <p className="text-[9px] text-red-600/70 dark:text-red-400/70">
          {t('errors.iterationLimit')}
        </p>
      )}
    </div>
  )
}
