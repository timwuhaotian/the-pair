import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { StatCard } from './ui/StatCard'

interface StatCardsProps {
  total: number
  running: number
  paused: number
  finished: number
  className?: string
}

export function StatCards({
  total,
  running,
  paused,
  finished,
  className
}: StatCardsProps): React.ReactNode {
  const { t } = useTranslation()

  return (
    <div className={cn('grid grid-cols-2 gap-3 md:grid-cols-4', className)}>
      <StatCard value={total} label={t('dashboard.stats.total')} color="primary" />
      <StatCard value={running} label={t('dashboard.stats.running')} color="green" />
      <StatCard value={paused} label={t('dashboard.stats.paused')} color="amber" />
      <StatCard value={finished} label={t('dashboard.stats.finished')} color="gray" />
    </div>
  )
}
