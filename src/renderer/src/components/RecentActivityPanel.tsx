import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { Activity, ActivityItem, type ActivityType } from './ActivityItem'
import { cn } from '../lib/utils'
import { emptyHint, sectionLabel } from '../lib/ui'

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
  return <div className={cn('flex flex-col overflow-hidden px-1 py-1', className)}>{children}</div>
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

  return (
    <Wrap className={className}>
      <h2 className={cn(sectionLabel, 'mb-2')}>{t('dashboard.activity.title')}</h2>
      {loading ? (
        <div role="status" className={cn(emptyHint, 'flex items-baseline justify-center gap-2')}>
          <span aria-hidden className="state-running tty-blink">
            ●
          </span>
          <span>{t('common.loading')}</span>
        </div>
      ) : activities.length === 0 ? (
        <p className={emptyHint}>{t('dashboard.activity.empty')}</p>
      ) : (
        <div className="flex-1 space-y-0 overflow-y-auto scrollbar-thin pr-1">
          {activities.map((activity) => (
            <ActivityItem key={`${activity.pairId}-${activity.timestamp}`} activity={activity} />
          ))}
        </div>
      )}
    </Wrap>
  )
}
