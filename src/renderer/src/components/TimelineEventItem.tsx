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

function getDotColor(type: TimelineEvent['type']): string {
  switch (type) {
    case 'mentor-plan':
    case 'mentor-review':
      return 'bg-blue-500 shadow-[0_0_0_2px_rgba(59,130,246,0.3)]'
    case 'executor-result':
      return 'bg-purple-500 shadow-[0_0_0_2px_rgba(168,85,247,0.3)]'
    case 'human-feedback':
      return 'bg-green-500 shadow-[0_0_0_2px_rgba(34,197,94,0.3)]'
    case 'acceptance-gate':
      return 'bg-amber-500 shadow-[0_0_0_2px_rgba(245,158,11,0.3)]'
    case 'handoff':
      return 'bg-slate-400 shadow-[0_0_0_2px_rgba(148,163,184,0.3)]'
  }
}

function getTitleColor(type: TimelineEvent['type']): string {
  switch (type) {
    case 'mentor-plan':
    case 'mentor-review':
      return 'text-blue-600 dark:text-blue-400'
    case 'executor-result':
      return 'text-purple-600 dark:text-purple-400'
    case 'human-feedback':
      return 'text-green-600 dark:text-green-400'
    case 'acceptance-gate':
      return 'text-amber-600 dark:text-amber-400'
    case 'handoff':
      return 'text-slate-500 dark:text-slate-400'
  }
}

export function TimelineEventItem({
  event,
  isFirst,
  isLast
}: TimelineEventItemProps): React.JSX.Element {
  return (
    <div className="relative flex gap-3 pb-3 last:pb-0">
      {/* Connector line */}
      <div className="relative flex w-6 shrink-0 flex-col items-center">
        {/* Top connector */}
        {!isFirst && <div className="w-px flex-1 bg-border/60" />}
        {/* Dot */}
        <div className={cn('h-2.5 w-2.5 shrink-0 rounded-full', getDotColor(event.type))} />
        {/* Bottom connector */}
        {!isLast && <div className="w-px flex-1 bg-border/60" />}
      </div>

      {/* Content */}
      <div className="min-w-0 flex-1 pt-px">
        <div className="flex items-center gap-2">
          <span className={cn('text-[11px] font-bold', getTitleColor(event.type))}>
            {event.type === 'mentor-plan'
              ? 'Mentor Plan'
              : event.type === 'executor-result'
                ? 'Executor'
                : event.type === 'mentor-review'
                  ? 'Review'
                  : event.type === 'human-feedback'
                    ? 'Human'
                    : event.type === 'acceptance-gate' && event.acceptanceVerdict
                      ? event.acceptanceVerdict.verdict.toUpperCase()
                      : 'Handoff'}
          </span>
          <span className="text-[9px] font-mono text-muted-foreground/40 tabular-nums">
            {formatTimestamp(event.timestamp)}
          </span>
          {event.durationMs != null && event.durationMs > 1000 && (
            <span className="text-[9px] text-muted-foreground/40">
              {formatDuration(event.durationMs)}
            </span>
          )}
          {event.from !== 'human' && <TokenChip usage={event.tokenUsage} compact />}
        </div>

        {/* Risk badge */}
        {event.acceptanceVerdict && (
          <div className="mt-1 flex items-center gap-1.5">
            <span className="rounded-full border border-border/40 bg-background/40 px-1.5 py-0.5 text-[8px] font-bold uppercase tracking-[0.16em] text-muted-foreground/70">
              {event.acceptanceVerdict.risk}
            </span>
          </div>
        )}

        {/* Summary */}
        {event.summary && (
          <p className="mt-1 text-[10px] leading-relaxed text-muted-foreground/70 line-clamp-2">
            {event.summary}
          </p>
        )}
      </div>
    </div>
  )
}
