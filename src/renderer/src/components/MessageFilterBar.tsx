import React from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'

type MessageFilter = 'all' | 'mentor' | 'executor'

interface MessageFilterBarProps {
  activeFilter: MessageFilter
  onFilterChange: (filter: MessageFilter) => void
  counts: { mentor: number; executor: number; all: number }
}

export function MessageFilterBar({
  activeFilter,
  onFilterChange,
  counts
}: MessageFilterBarProps): React.ReactNode {
  const { t } = useTranslation()
  const filters: { key: MessageFilter; labelKey: string; color: string }[] = [
    { key: 'all', labelKey: 'console.all', color: 'text-muted-foreground' },
    { key: 'mentor', labelKey: 'common.mentor', color: 'text-blue-600 dark:text-blue-400' },
    { key: 'executor', labelKey: 'common.executor', color: 'text-purple-600 dark:text-purple-400' }
  ]

  return (
    <div className="flex items-center gap-1">
      {filters.map(({ key, labelKey, color }) => (
        <button
          key={key}
          onClick={() => onFilterChange(key)}
          className={cn(
            'flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[10px] font-medium transition-all cursor-pointer',
            activeFilter === key
              ? key === 'all'
                ? 'bg-primary/15 text-primary border border-primary/25'
                : key === 'mentor'
                  ? 'bg-blue-500/15 text-blue-700 dark:text-blue-300 border border-blue-500/25'
                  : 'bg-purple-500/15 text-purple-700 dark:text-purple-300 border border-purple-500/25'
              : 'text-muted-foreground hover:bg-muted/40 border border-transparent'
          )}
        >
          <span className={activeFilter === key ? color : ''}>{t(labelKey)}</span>
          <span
            className={cn(
              'rounded-full px-1.5 py-0.5 text-[9px]',
              activeFilter === key ? 'bg-background/60' : 'bg-muted/40'
            )}
          >
            {key === 'all' ? counts.all : counts[key]}
          </span>
        </button>
      ))}
    </div>
  )
}
