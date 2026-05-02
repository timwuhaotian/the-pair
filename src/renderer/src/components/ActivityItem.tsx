import { CheckCircle, ArrowRightLeft, FileText } from 'lucide-react'

export type ActivityType = 'status_change' | 'handoff' | 'result' | 'acceptance'

export interface Activity {
  pairId: string
  pairName: string
  type: ActivityType
  description: string
  timestamp: number
  role?: string
}

interface ActivityItemProps {
  activity: Activity
  className?: string
}

const iconMap: Record<ActivityType, React.ReactNode> = {
  status_change: <CheckCircle size={14} className="text-green-500" />,
  handoff: <ArrowRightLeft size={14} className="text-blue-500" />,
  result: <FileText size={14} className="text-purple-500" />,
  acceptance: <CheckCircle size={14} className="text-amber-500" />
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

export function ActivityItem({ activity, className }: ActivityItemProps): React.ReactNode {
  return (
    <div className={`flex items-start gap-2 py-2 ${className || ''}`}>
      <span className="mt-0.5 shrink-0">{iconMap[activity.type]}</span>
      <div className="min-w-0 flex-1">
        <div className="text-xs text-foreground">
          <span className="font-medium">{activity.pairName}</span>
          <span className="line-clamp-2 text-muted-foreground"> — {activity.description}</span>
        </div>
        <div className="text-[10px] text-muted-foreground">
          {formatRelativeTime(activity.timestamp)}
        </div>
      </div>
    </div>
  )
}
