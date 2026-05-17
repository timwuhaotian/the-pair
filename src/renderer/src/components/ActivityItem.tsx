import { cn } from '../lib/utils'

export type ActivityType = 'status_change' | 'handoff' | 'result' | 'acceptance'

export interface Activity {
  pairId: string
  pairName: string
  type: ActivityType
  description: string
  timestamp: number
  role?: string
}

const GLYPH: Record<ActivityType, { char: string; tone: string }> = {
  status_change: { char: '✓', tone: 'state-done' },
  handoff: { char: '⇄', tone: 'role-mentor' },
  result: { char: '▸', tone: 'role-executor' },
  acceptance: { char: '◇', tone: 'state-running' }
}

function formatRelativeTime(timestamp: number): string {
  const now = Date.now()
  const diff = now - timestamp
  const minutes = Math.floor(diff / 60000)
  const hours = Math.floor(diff / 3600000)

  if (minutes < 1) return 'just now'
  if (minutes < 60) return `${minutes}m ago`
  if (hours < 24) return `${hours}h ago`
  return `${Math.floor(hours / 24)}d ago`
}

interface ActivityItemProps {
  activity: Activity
  className?: string
}

export function ActivityItem({ activity, className }: ActivityItemProps): React.ReactNode {
  const g = GLYPH[activity.type]
  return (
    <div className={cn('flex items-baseline gap-2 py-1 font-mono text-[11px]', className)}>
      <span aria-hidden className={cn('shrink-0 select-none w-[1ch] tabular-nums', g.tone)}>
        {g.char}
      </span>
      <div className="min-w-0 flex-1">
        <div className="text-foreground/85">
          <span className="font-bold">{activity.pairName}</span>
          <span className="line-clamp-2 text-muted-foreground"> — {activity.description}</span>
        </div>
        <div className="text-[10px] text-muted-foreground-faint">
          · {formatRelativeTime(activity.timestamp)}
        </div>
      </div>
    </div>
  )
}
