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
  const filters: { key: MessageFilter; labelKey: string; activeClass: string }[] = [
    {
      key: 'all',
      labelKey: 'console.all',
      activeClass: 'text-foreground bg-foreground/10 border-foreground/30'
    },
    {
      key: 'mentor',
      labelKey: 'common.mentor',
      activeClass: 'role-mentor bg-role-mentor border-role-mentor'
    },
    {
      key: 'executor',
      labelKey: 'common.executor',
      activeClass: 'role-executor bg-role-executor border-role-executor'
    }
  ]

  return (
    <div className="flex items-center gap-1 font-mono">
      {filters.map(({ key, labelKey, activeClass }) => {
        const isActive = activeFilter === key
        return (
          <button
            key={key}
            onClick={() => onFilterChange(key)}
            data-testid={`filter-${key}`}
            className={cn(
              'inline-flex items-baseline gap-1 rounded-sm border px-1.5 py-px text-[10px] uppercase tracking-[0.12em] transition-colors cursor-pointer',
              isActive
                ? activeClass
                : 'border-transparent text-muted-foreground hover:bg-foreground/[0.04] hover:text-foreground/80'
            )}
          >
            <span>{t(labelKey)}</span>
            <span className={cn('tabular-nums', !isActive && 'text-muted-foreground-faint')}>
              {key === 'all' ? counts.all : counts[key]}
            </span>
          </button>
        )
      })}
    </div>
  )
}
