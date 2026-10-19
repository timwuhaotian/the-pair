import React from 'react'
import { useTranslation } from 'react-i18next'
import { type Pair } from '../store/usePairStore'
import { emptyHint, sectionLabel } from '../lib/ui'
import { InsightsPanel } from './InsightsPanel'
import { RecentActivityPanel } from './RecentActivityPanel'

import { buildPairGroups } from '../lib/dashboardPairs'
import { StatusBadge } from './StatusBadge'

interface ActivityViewProps {
  pairs: Pair[]
  onSelectPair?: (pair: Pair) => void
}

export function ActivityView({ pairs, onSelectPair }: ActivityViewProps): React.ReactNode {
  const { t } = useTranslation()
  const attentionPairs =
    buildPairGroups(pairs).find((group) => group.key === 'attention')?.pairs ?? []

  return (
    <div data-testid="activity-view" className="flex h-full flex-col overflow-hidden">
      <div className="shrink-0 px-6 pt-5">
        <h1 className="text-[17px] font-semibold tracking-tight text-foreground">
          {t('shell.activity')}
        </h1>
        <p className="mt-0.5 text-[13px] text-muted-foreground">{t('activity.subtitle')}</p>
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-1 content-start gap-4 overflow-y-auto px-6 py-4 scrollbar-thin lg:grid-cols-2">
        <section
          aria-label={t('dashboard.insights.needsAttention')}
          className="shell-card flex flex-col gap-2 px-4 py-3"
        >
          <h2 className={sectionLabel}>{t('dashboard.insights.needsAttention')}</h2>
          {attentionPairs.length === 0 ? (
            <p className={emptyHint}>{t('dashboard.insights.noAttention')}</p>
          ) : (
            <div className="space-y-1.5">
              {attentionPairs.slice(0, 8).map((pair) => (
                <button
                  key={pair.id}
                  type="button"
                  onClick={() => onSelectPair?.(pair)}
                  className="w-full min-w-0 cursor-pointer rounded-md border-l-2 border-state-error/40 bg-state-error/[0.04] px-2.5 py-1.5 text-left transition-colors hover:bg-state-error/[0.08]"
                >
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="truncate text-[13px] font-medium text-foreground/90">
                      {pair.name}
                    </span>
                    <StatusBadge status={pair.status} />
                  </div>
                  <div className="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">
                    {pair.directory}
                  </div>
                </button>
              ))}
            </div>
          )}
        </section>
        <RecentActivityPanel className="shell-card min-h-[280px] border px-4 py-3" />
        {attentionPairs.length > 0 && (
          <p className="text-[13px] text-muted-foreground lg:col-span-2">
            {t('activity.attentionHint', { count: attentionPairs.length })}
          </p>
        )}
      </div>
    </div>
  )
}

export function InsightsView(): React.ReactNode {
  const { t } = useTranslation()

  return (
    <div data-testid="insights-view" className="flex h-full flex-col overflow-hidden">
      <div className="shrink-0 px-6 pt-5">
        <h1 className="text-[17px] font-semibold tracking-tight text-foreground">
          {t('shell.insights')}
        </h1>
        <p className="mt-0.5 text-[13px] text-muted-foreground">{t('insights.subtitle')}</p>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4 scrollbar-thin">
        <InsightsPanel className="shell-card max-w-[720px] px-4 py-3" />
      </div>
    </div>
  )
}
