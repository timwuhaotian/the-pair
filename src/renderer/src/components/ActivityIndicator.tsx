/* eslint-disable react-hooks/purity */
import React, { useMemo } from 'react'
import { AlertTriangle, Loader2 } from 'lucide-react'
import { cn } from '../lib/utils'
import type { AgentActivity } from '../store/usePairStore'

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds}s`
  const mins = Math.floor(seconds / 60)
  const secs = seconds % 60
  return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`
}

interface ActivityIndicatorProps {
  activity: AgentActivity
  className?: string
}

export function ActivityIndicator({
  activity,
  className
}: ActivityIndicatorProps): React.ReactNode {
  const elapsed = useMemo(
    () => (activity.startedAt ? Math.floor((Date.now() - activity.startedAt) / 1000) : 0),
    [activity.startedAt, activity.updatedAt]
  )
  const lastOutputAge = useMemo(
    () => (activity.lastOutputAt ? Math.floor((Date.now() - activity.lastOutputAt) / 1000) : 0),
    [activity.lastOutputAt, activity.updatedAt]
  )

  if (activity.phase === 'idle' || activity.phase === 'waiting') {
    return null
  }

  if (activity.phase === 'stalled') {
    return (
      <div
        className={cn(
          'inline-flex items-center gap-1.5 rounded-full border border-red-500/40 bg-red-500/10 px-2 py-0.5',
          className
        )}
      >
        <AlertTriangle size={10} className="text-red-400" />
        <span className="text-[9px] font-medium text-red-400">
          卡住{lastOutputAge > 0 && ` · ${formatDuration(lastOutputAge)} 无输出`}
        </span>
      </div>
    )
  }

  if (activity.phase === 'error') {
    return (
      <div
        className={cn(
          'inline-flex items-center gap-1.5 rounded-full border border-red-500/30 bg-red-500/8 px-2 py-0.5',
          className
        )}
      >
        <span className="text-[9px] font-medium text-red-400">错误</span>
      </div>
    )
  }

  if (activity.phase === 'thinking') {
    return (
      <div
        className={cn(
          'inline-flex items-center gap-1.5 rounded-full border border-blue-500/25 bg-blue-500/8 px-2 py-0.5',
          className
        )}
      >
        <Loader2 size={9} className="animate-spin text-blue-400" />
        <span className="text-[9px] font-medium text-blue-400">
          推理{elapsed > 0 && ` · ${formatDuration(elapsed)}`}
        </span>
      </div>
    )
  }

  if (activity.phase === 'using_tools') {
    const label = activity.detail?.trim() || activity.label.trim() || '工具调用'
    return (
      <div
        className={cn(
          'inline-flex items-center gap-1.5 rounded-full border border-amber-500/25 bg-amber-500/8 px-2 py-0.5',
          className
        )}
      >
        <span className="text-[9px] font-medium text-amber-400">
          {label}
          {activity.outputLineCount !== undefined && activity.outputLineCount > 0
            ? ` · ${activity.outputLineCount} 行`
            : ''}
        </span>
      </div>
    )
  }

  if (activity.phase === 'responding') {
    return (
      <div
        className={cn(
          'inline-flex items-center gap-1.5 rounded-full border border-purple-500/25 bg-purple-500/8 px-2 py-0.5',
          className
        )}
      >
        <span className="text-[9px] font-medium text-purple-400">
          输出
          {activity.outputLineCount !== undefined && activity.outputLineCount > 0
            ? ` · ${activity.outputLineCount} 行`
            : ''}
          {elapsed > 0 && ` · ${formatDuration(elapsed)}`}
          {lastOutputAge > 5 && elapsed > 0 ? ` · 最后活动 ${formatDuration(lastOutputAge)}前` : ''}
        </span>
      </div>
    )
  }

  return null
}
