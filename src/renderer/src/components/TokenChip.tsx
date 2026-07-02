import { memo } from 'react'
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
    ? `output: ${outputCount.toLocaleString()} tokens${inputCount > 0 ? `\ninput: ${inputCount.toLocaleString()} tokens` : ''}\ntotal: ${totalCount.toLocaleString()} tokens\nsource: ${usage.source}${usage.provider ? `\nprovider: ${usage.provider}` : ''}`
    : 'waiting for token data…'

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
      <span className="text-muted-foreground-faint">tok</span>
    </span>
  )
})
