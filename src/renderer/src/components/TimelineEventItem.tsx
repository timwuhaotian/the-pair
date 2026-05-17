import React from 'react'
import { cn } from '../lib/utils'
import { TokenChip } from './TokenChip'
import type { TimelineEvent } from '../lib/timeline'
import { formatTimestamp, formatDuration } from '../lib/timeline'

interface TimelineEventItemProps {
  event: TimelineEvent
  isFirst: boolean
  isLast: boolean
}

function eventGlyph(type: TimelineEvent['type']): { char: string; tone: string } {
  switch (type) {
    case 'mentor-plan':
    case 'mentor-review':
      return { char: '●', tone: 'role-mentor' }
    case 'executor-result':
      return { char: '●', tone: 'role-executor' }
    case 'human-feedback':
      return { char: '>', tone: 'role-human' }
    case 'acceptance-gate':
      return { char: '◇', tone: 'state-running' }
    case 'handoff':
      return { char: '·', tone: 'text-muted-foreground-faint' }
  }
}

function eventLabel(event: TimelineEvent): string {
  switch (event.type) {
    case 'mentor-plan':
      return 'mentor·plan'
    case 'mentor-review':
      return 'mentor·review'
    case 'executor-result':
      return 'executor'
    case 'human-feedback':
      return 'human'
    case 'acceptance-gate':
      return event.acceptanceVerdict
        ? `[${event.acceptanceVerdict.verdict.toLowerCase()}]`
        : 'acceptance'
    case 'handoff':
      return '· handoff'
  }
}

export function TimelineEventItem({
  event,
  isFirst,
  isLast
}: TimelineEventItemProps): React.JSX.Element {
  const g = eventGlyph(event.type)
  return (
    <div className="relative flex gap-2 pb-2 last:pb-0 font-mono">
      <div className="relative flex w-3 shrink-0 flex-col items-center">
        {!isFirst && <div className="w-px flex-1 bg-border/55" />}
        <span aria-hidden className={cn('select-none tabular-nums leading-none mt-0.5', g.tone)}>
          {g.char}
        </span>
        {!isLast && <div className="w-px flex-1 bg-border/55" />}
      </div>

      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2 flex-wrap">
          <span className={cn('text-[11px] font-bold', g.tone)}>{eventLabel(event)}</span>
          <span className="text-[9px] tabular-nums text-muted-foreground-faint">
            {formatTimestamp(event.timestamp)}
          </span>
          {event.durationMs != null && event.durationMs > 1000 && (
            <span className="text-[9px] text-muted-foreground-faint">
              {formatDuration(event.durationMs)}
            </span>
          )}
          {event.from !== 'human' && <TokenChip usage={event.tokenUsage} compact />}
        </div>

        {event.acceptanceVerdict && (
          <div className="mt-0.5 flex items-baseline gap-1.5">
            <span className="border border-border px-1 py-px text-[8px] uppercase tracking-[0.14em] text-muted-foreground">
              {event.acceptanceVerdict.risk}
            </span>
          </div>
        )}

        {event.summary && (
          <p className="mt-0.5 text-[10px] leading-relaxed text-muted-foreground line-clamp-2 [overflow-wrap:anywhere]">
            {event.summary}
          </p>
        )}
      </div>
    </div>
  )
}
