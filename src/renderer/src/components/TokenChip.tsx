import { cn } from '../lib/utils'
import type { TurnTokenUsage } from '../types'

interface TokenChipProps {
  usage?: TurnTokenUsage
  isLive?: boolean
  compact?: boolean
  className?: string
}

function formatTokenCount(count: number): string {
  if (count >= 1000000) {
    return `${(count / 1000000).toFixed(1)}M`
  }
  if (count >= 1000) {
    return `${(count / 1000).toFixed(1)}k`
  }
  return count.toString()
}

export function TokenChip({ usage, isLive, compact, className }: TokenChipProps) {
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
    ? `Output: ${outputCount.toLocaleString()} tokens${inputCount > 0 ? `\nInput: ${inputCount.toLocaleString()} tokens` : ''}\nTotal: ${totalCount.toLocaleString()} tokens\nSource: ${usage.source}${usage.provider ? `\nProvider: ${usage.provider}` : ''}`
    : 'Waiting for token data...'

  if (compact) {
    return (
      <span
        className={cn(
          'inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] font-medium',
          'bg-muted dark:bg-white/8 rounded-md',
          'text-muted-foreground dark:text-white/60',
          className
        )}
        title={tooltipText}
      >
        {showLiveIndicator && (
          <span className="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse" />
        )}
        {displayCount} tok
      </span>
    )
  }

  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 px-2 py-0.5 text-xs font-medium',
        'bg-muted dark:bg-white/8 rounded-full',
        'text-muted-foreground dark:text-white/60',
        'transition-colors duration-200',
        showLiveIndicator && 'bg-green-50 dark:bg-green-900/20 text-green-700 dark:text-green-400',
        className
      )}
      title={tooltipText}
    >
      {showLiveIndicator && (
        <span className="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse" />
      )}
      {displayCount} tok
    </span>
  )
}
