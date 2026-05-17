/* eslint-disable react-hooks/purity */
import React, { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
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
  const { t } = useTranslation()
  const elapsed = useMemo(
    () => (activity.startedAt ? Math.floor((Date.now() - activity.startedAt) / 1000) : 0),
    [activity.startedAt]
  )
  const lastOutputAge = useMemo(
    () => (activity.lastOutputAt ? Math.floor((Date.now() - activity.lastOutputAt) / 1000) : 0),
    [activity.lastOutputAt]
  )

  if (activity.phase === 'idle' || activity.phase === 'waiting') return null

  const base = 'inline-flex items-baseline gap-1 font-mono text-[10px] uppercase tracking-[0.14em]'

  if (activity.phase === 'stalled') {
    return (
      <span className={cn(base, 'state-error tty-blink', className)}>
        <span aria-hidden>!</span>
        <span>
          {t('activity.stalled')}
          {lastOutputAge > 0 && ` · ${formatDuration(lastOutputAge)} ${t('activity.noOutput')}`}
        </span>
      </span>
    )
  }

  if (activity.phase === 'error') {
    return (
      <span className={cn(base, 'state-error', className)}>
        <span aria-hidden>✗</span>
        <span>{t('activity.error')}</span>
      </span>
    )
  }

  if (activity.phase === 'thinking') {
    return (
      <span className={cn(base, 'role-mentor', className)}>
        <span aria-hidden className="tty-blink">
          *
        </span>
        <span>
          {t('activity.thinking')}
          {elapsed > 0 && ` · ${formatDuration(elapsed)}`}
        </span>
      </span>
    )
  }

  if (activity.phase === 'using_tools') {
    const label = activity.detail?.trim() || activity.label.trim() || t('activity.toolCall')
    return (
      <span className={cn(base, 'state-running', className)}>
        <span aria-hidden className="tty-blink">
          *
        </span>
        <span>
          {label}
          {activity.outputLineCount !== undefined && activity.outputLineCount > 0
            ? ` · ${activity.outputLineCount} ${t('activity.lines')}`
            : ''}
        </span>
      </span>
    )
  }

  if (activity.phase === 'responding') {
    return (
      <span className={cn(base, 'role-executor', className)}>
        <span aria-hidden className="tty-blink">
          *
        </span>
        <span>
          {t('activity.output')}
          {activity.outputLineCount !== undefined && activity.outputLineCount > 0
            ? ` · ${activity.outputLineCount} ${t('activity.lines')}`
            : ''}
          {elapsed > 0 && ` · ${formatDuration(elapsed)}`}
          {lastOutputAge > 5 && elapsed > 0
            ? ` · ${t('activity.lastActivityAgo', { time: formatDuration(lastOutputAge) })}`
            : ''}
        </span>
      </span>
    )
  }

  return null
}
