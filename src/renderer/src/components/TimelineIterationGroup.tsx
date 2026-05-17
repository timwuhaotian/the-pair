import React from 'react'
import { cn } from '../lib/utils'
import { TimelineEventItem } from './TimelineEventItem'
import type { IterationGroup } from '../lib/timeline'
import { formatDuration, formatTokenCount } from '../lib/timeline'

interface TimelineIterationGroupProps {
  group: IterationGroup
  isLast: boolean
}

export function TimelineIterationGroup({
  group,
  isLast
}: TimelineIterationGroupProps): React.JSX.Element {
  return (
    <div className={cn('font-mono', !isLast && 'pb-3 border-b border-border/30 mb-3')}>
      <div className="mb-1 flex items-baseline gap-2 text-[10px] uppercase tracking-[0.16em]">
        <span className="text-foreground/80">── iter {group.iteration}</span>
        <span className="text-muted-foreground-faint tabular-nums">
          {formatDuration(group.durationMs)}
        </span>
        {group.totalTokens > 0 && (
          <span className="text-muted-foreground-faint tabular-nums">
            · {formatTokenCount(group.totalTokens)} tok
          </span>
        )}
      </div>

      <div className="pl-[2ch]">
        {group.events.map((event, idx) => (
          <TimelineEventItem
            key={event.id}
            event={event}
            isFirst={idx === 0}
            isLast={idx === group.events.length - 1}
          />
        ))}
      </div>
    </div>
  )
}
