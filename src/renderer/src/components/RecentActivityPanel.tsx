import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { Activity, ActivityItem, type ActivityType } from './ActivityItem'
import { cn } from '../lib/utils'

interface RecentActivityPanelProps {
  className?: string
}

function Wrap({
  className,
  children
}: {
  className?: string
  children: React.ReactNode
}): React.ReactNode {
  return (
    <div
      className={cn(
        'flex flex-col border border-border bg-background/40 overflow-hidden p-3 font-mono',
        className
      )}
    >
      {children}
    </div>
  )
}

export function RecentActivityPanel({ className }: RecentActivityPanelProps): React.ReactNode {
  const { t } = useTranslation()
  const [activities, setActivities] = useState<Activity[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let cancelled = false

    const loadActivities = async (): Promise<void> => {
      try {
        const result = (await invoke('get_recent_activities', { limit: 10 })) as Array<{
          pair_id: string
          pair_name: string
          activity_type: ActivityType
          description: string
          timestamp: number
          role?: string
        }>

        if (cancelled) return
        setActivities(
          result.map((a) => ({
            pairId: a.pair_id,
            pairName: a.pair_name,
            type: a.activity_type,
            description: a.description,
            timestamp: a.timestamp,
            role: a.role
          }))
        )
      } catch (error) {
        console.warn('[RecentActivityPanel] Failed to load activities:', error)
      } finally {
        if (!cancelled) setLoading(false)
      }
    }

    void loadActivities()
    const interval = setInterval(() => void loadActivities(), 10000)
    return () => {
      cancelled = true
      clearInterval(interval)
    }
  }, [])

  if (loading) {
    return (
      <Wrap className={className}>
        <h2 className="mb-2 text-[10px] uppercase tracking-[0.18em] text-foreground/85">
          <span className="text-primary">──</span> {t('dashboard.activity.title')}{' '}
          <span className="text-primary">──</span>
        </h2>
        <div className="flex items-baseline gap-2 py-4 text-[11px] text-muted-foreground">
          <span className="state-running tty-blink">●</span>
          <span>· {t('common.loading')}</span>
        </div>
      </Wrap>
    )
  }

  if (activities.length === 0) {
    return (
      <Wrap className={className}>
        <h2 className="mb-2 text-[10px] uppercase tracking-[0.18em] text-foreground/85">
          <span className="text-primary">──</span> {t('dashboard.activity.title')}{' '}
          <span className="text-primary">──</span>
        </h2>
        <p className="py-4 text-[11px] text-muted-foreground-faint">
          — {t('dashboard.activity.empty')} —
        </p>
      </Wrap>
    )
  }

  return (
    <Wrap className={className}>
      <h2 className="mb-2 text-[10px] uppercase tracking-[0.18em] text-foreground/85">
        <span className="text-primary">──</span> {t('dashboard.activity.title')}{' '}
        <span className="text-primary">──</span>
      </h2>
      <div className="flex-1 space-y-0 overflow-y-auto scrollbar-thin pr-1">
        {activities.map((activity) => (
          <ActivityItem key={`${activity.pairId}-${activity.timestamp}`} activity={activity} />
        ))}
      </div>
    </Wrap>
  )
}
