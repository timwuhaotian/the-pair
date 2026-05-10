import { AlertTriangle, Cpu, FolderKanban, GitBranch, MemoryStick, Radio } from 'lucide-react'
import { useTranslation } from 'react-i18next'
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

  const metrics = [
    {
      label: t('dashboard.insights.attention'),
      value: insights.needsAttention,
      icon: <AlertTriangle size={14} />
    },
    {
      label: t('dashboard.insights.running'),
      value: insights.running,
      icon: <Radio size={14} />
    },
    {
      label: t('dashboard.insights.cpu'),
      value: `${insights.cpuUsage}%`,
      icon: <Cpu size={14} />
    },
    {
      label: t('dashboard.insights.memory'),
      value: `${insights.memUsage} MB`,
      icon: <MemoryStick size={14} />
    }
  ]

  return (
    <aside
      className={`glass-card flex h-full flex-col gap-4 overflow-hidden rounded-lg border border-border/70 bg-card/80 p-4 shadow-sm ${className || ''}`}
    >
      <div>
        <h2 className="text-sm font-semibold text-foreground">{t('dashboard.insights.title')}</h2>
        <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
          {t('dashboard.insights.description')}
        </p>
      </div>

      <div className="grid grid-cols-2 gap-2">
        {metrics.map((metric) => (
          <div
            key={metric.label}
            className="rounded-lg border border-border/60 bg-background/60 p-3"
          >
            <div className="flex items-center gap-1.5 text-muted-foreground">
              {metric.icon}
              <span className="text-[11px] font-medium uppercase tracking-wide">
                {metric.label}
              </span>
            </div>
            <div className="mt-2 text-lg font-semibold tabular-nums text-foreground">
              {metric.value}
            </div>
          </div>
        ))}
      </div>

      <div className="rounded-lg border border-border/60 bg-background/60 p-3">
        <div className="mb-3 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          <FolderKanban size={14} />
          {t('dashboard.insights.workspaces')}
        </div>
        <div className="grid grid-cols-2 gap-2 text-sm">
          <div>
            <div className="font-semibold text-foreground">{insights.workspaceCount}</div>
            <div className="text-xs text-muted-foreground">
              {t('dashboard.insights.workspaces')}
            </div>
          </div>
          <div>
            <div className="flex items-center gap-1 font-semibold text-foreground">
              <GitBranch size={14} />
              {insights.modifiedFiles}
            </div>
            <div className="text-xs text-muted-foreground">
              {t('dashboard.insights.changedFiles')}
            </div>
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1 rounded-lg border border-border/60 bg-background/60 p-3">
        <h3 className="mb-3 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          {t('dashboard.insights.needsAttention')}
        </h3>
        {attentionPairs.length === 0 ? (
          <p className="text-sm leading-relaxed text-muted-foreground">
            {t('dashboard.insights.noAttention')}
          </p>
        ) : (
          <div className="space-y-2">
            {attentionPairs.slice(0, 4).map((pair) => (
              <div key={pair.id} className="min-w-0 rounded-md border border-border/50 p-2">
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate text-sm font-medium text-foreground">{pair.name}</span>
                  <StatusBadge status={pair.status} />
                </div>
                <div className="mt-1 truncate text-[11px] text-muted-foreground">
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
