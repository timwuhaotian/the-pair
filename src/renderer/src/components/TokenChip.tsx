import { memo } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import type { TurnTokenUsage } from '../types'

interface TokenChipProps {
  usage?: TurnTokenUsage
  isLive?: boolean
  compact?: boolean
  className?: string
}

function formatTokenCount(count: number): string {
  if (count >= 1000000) return `${(count / 1000000).toFixed(1)}M`
  if (count >= 1000) return `${(count / 1000).toFixed(1)}k`
  return count.toString()
}

export const TokenChip = memo(function TokenChip({
  usage,
  isLive,
  compact,
  className
}: TokenChipProps): React.ReactNode {
  const { t } = useTranslation()
  const hasUsage = !!usage
  const outputCount = usage?.outputTokens ?? 0
  const inputCount = usage?.inputTokens ?? 0
  const totalCount = outputCount + inputCount
  const displayCount =
    hasUsage && totalCount > 0
      ? formatTokenCount(totalCount)
      : hasUsage
        ? formatTokenCount(outputCount)
        : '—'
  const showLiveIndicator = isLive && usage?.source === 'live'

  const tooltipText = hasUsage
    ? [
        t('console.tokTooltipOutput', { value: outputCount.toLocaleString() }),
        inputCount > 0 && t('console.tokTooltipInput', { value: inputCount.toLocaleString() }),
        t('console.tokTooltipTotal', { value: totalCount.toLocaleString() }),
        t('console.tokTooltipSource', { value: usage.source }),
        usage.provider && t('console.tokTooltipProvider', { value: usage.provider })
      ]
        .filter(Boolean)
        .join('\n')
    : t('console.tokWaiting')

  return (
    <span
      className={cn(
        'inline-flex items-baseline gap-1 font-mono tabular-nums',
        compact ? 'text-[10px]' : 'text-[11px]',
        showLiveIndicator ? 'state-done' : 'text-muted-foreground',
        className
      )}
      title={tooltipText}
    >
      {showLiveIndicator && (
        <span aria-hidden className="state-done tty-blink select-none">
          ●
        </span>
      )}
      <span>{displayCount}</span>
      <span className="text-muted-foreground-faint">{t('console.tokUnit')}</span>
    </span>
  )
})
