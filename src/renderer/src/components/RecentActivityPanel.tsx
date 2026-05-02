import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { invoke } from '@tauri-apps/api/core'
import { Activity, ActivityItem, type ActivityType } from './ActivityItem'

interface RecentActivityPanelProps {
  className?: string
}

export function RecentActivityPanel({ className }: RecentActivityPanelProps): React.ReactNode {
  const { t } = useTranslation()
  const [activities, setActivities] = useState<Activity[]>([])
  const [loading, setLoading] = useState(true)

  const loadActivities = async () => {
    try {
      const result = (await invoke('get_recent_activities', {
        limit: 10
      })) as Array<{
        pair_id: string
        pair_name: string
        activity_type: ActivityType
        description: string
        timestamp: number
        role?: string
      }>

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
      setLoading(false)
    }
  }

  useEffect(() => {
    void loadActivities()
    const interval = setInterval(() => void loadActivities(), 10000)
    return () => clearInterval(interval)
  }, [])

  if (loading) {
    return (
      <div className="flex flex-col overflow-hidden">
        <h2 className="mb-3 text-sm font-semibold text-foreground">
          {t('dashboard.activity.title')}
        </h2>
        <div className="flex items-center justify-center p-4 text-sm text-muted-foreground">
          Loading...
        </div>
      </div>
    )
  }

  if (activities.length === 0) {
    return (
      <div className="flex flex-col overflow-hidden">
        <h2 className="mb-3 text-sm font-semibold text-foreground">
          {t('dashboard.activity.title')}
        </h2>
        <div className="flex flex-1 items-center justify-center p-6 text-center">
          <p className="text-sm text-muted-foreground">{t('dashboard.activity.empty')}</p>
        </div>
      </div>
    )
  }

  return (
    <div className={`flex flex-col overflow-hidden ${className || ''}`}>
      <h2 className="mb-3 text-sm font-semibold text-foreground">
        {t('dashboard.activity.title')}
      </h2>
      <div className="flex-1 space-y-0 overflow-y-auto scrollbar-thin pr-1">
        {activities.map((activity) => (
          <ActivityItem key={`${activity.pairId}-${activity.timestamp}`} activity={activity} />
        ))}
      </div>
    </div>
  )
}
