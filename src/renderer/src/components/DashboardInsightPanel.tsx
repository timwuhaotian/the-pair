import { AlertTriangle, Cpu, GitBranch, MemoryStick, Radio } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { type Pair } from '../store/usePairStore'
import { buildPairGroups, buildPairInsights } from '../lib/dashboardPairs'
import { StatusBadge } from './StatusBadge'

interface DashboardInsightPanelProps {
  pairs: Pair[]
  className?: string
}

export function DashboardInsightPanel({
  pairs,
  className
}: DashboardInsightPanelProps): React.ReactNode {
  const { t } = useTranslation()
  const insights = buildPairInsights(pairs)
  const attentionPairs =
    buildPairGroups(pairs).find((group) => group.key === 'attention')?.pairs ?? []

  const metrics: Array<{
    label: string
    value: string | number
    tone: string
    icon: React.ReactNode
  }> = [
    {
      label: t('dashboard.insights.attention'),
      value: insights.needsAttention,
      tone: insights.needsAttention > 0 ? 'state-error' : 'text-muted-foreground',
      icon: <AlertTriangle size={11} />
    },
    {
      label: t('dashboard.insights.running'),
      value: insights.running,
      tone: insights.running > 0 ? 'state-done' : 'text-muted-foreground',
      icon: <Radio size={11} />
    },
    {
      label: t('dashboard.insights.cpu'),
      value: `${insights.cpuUsage}%`,
      tone: 'text-foreground/90',
      icon: <Cpu size={11} />
    },
    {
      label: t('dashboard.insights.memory'),
      value: `${insights.memUsage} MB`,
      tone: 'text-foreground/90',
      icon: <MemoryStick size={11} />
    }
  ]

  return (
    <aside
      className={cn(
        'flex h-full flex-col gap-3 overflow-hidden border-l border-border bg-background/40 px-4 py-3 font-mono',
        className
      )}
    >
      <div>
        <h2 className="text-[11px] uppercase tracking-[0.18em] text-foreground">
          <span className="text-primary">──</span> {t('dashboard.insights.title')}{' '}
          <span className="text-primary">──</span>
        </h2>
        <p className="mt-1 text-[10px] leading-relaxed text-muted-foreground">
          · {t('dashboard.insights.description')}
        </p>
      </div>

      <div className="grid grid-cols-2 gap-1.5">
        {metrics.map((metric) => (
          <div key={metric.label} className="border border-border bg-background/40 px-2 py-1.5">
            <div className="flex items-baseline gap-1 text-[10px] uppercase tracking-[0.14em] text-muted-foreground">
              <span className="translate-y-px">{metric.icon}</span>
              <span>{metric.label}</span>
            </div>
            <div
              className={cn('mt-0.5 text-[15px] tabular-nums font-bold leading-none', metric.tone)}
            >
              {metric.value}
            </div>
          </div>
        ))}
      </div>

      <div className="border border-border bg-background/40 px-3 py-2">
        <div className="mb-2 flex items-baseline gap-1.5 text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
          <span aria-hidden className="text-muted-foreground-faint">
            ▸
          </span>
          {t('dashboard.insights.workspaces')}
        </div>
        <div className="grid grid-cols-2 gap-2 text-[11px]">
          <div>
            <div className="text-foreground/90 tabular-nums font-bold">
              {insights.workspaceCount}
            </div>
            <div className="text-[10px] text-muted-foreground">
              {t('dashboard.insights.workspaces')}
            </div>
          </div>
          <div>
            <div className="flex items-baseline gap-1 text-foreground/90 tabular-nums font-bold">
              <GitBranch size={11} className="translate-y-px" />
              {insights.modifiedFiles}
            </div>
            <div className="text-[10px] text-muted-foreground">
              {t('dashboard.insights.changedFiles')}
            </div>
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1 border border-border bg-background/40 px-3 py-2 overflow-y-auto scrollbar-thin">
        <h3 className="mb-2 text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
          <span className="text-primary">──</span> {t('dashboard.insights.needsAttention')}{' '}
          <span className="text-primary">──</span>
        </h3>
        {attentionPairs.length === 0 ? (
          <p className="text-[11px] leading-relaxed text-muted-foreground-faint">
            — {t('dashboard.insights.noAttention')} —
          </p>
        ) : (
          <div className="space-y-1">
            {attentionPairs.slice(0, 4).map((pair) => (
              <div
                key={pair.id}
                className="min-w-0 border-l-2 border-state-error/40 bg-state-error/[0.04] px-2 py-1"
              >
                <div className="flex items-baseline justify-between gap-2">
                  <span className="truncate text-[12px] text-foreground/90">{pair.name}</span>
                  <StatusBadge status={pair.status} />
                </div>
                <div className="mt-0.5 truncate text-[10px] text-muted-foreground">
                  {pair.directory}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </aside>
  )
}
