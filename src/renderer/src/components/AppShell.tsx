import React from 'react'
import { useTranslation } from 'react-i18next'
import { BarChart3, Clock3, Layers } from 'lucide-react'
import { cn } from '../lib/utils'
import { activeTint, hoverTint } from '../lib/ui'
import { usePairStore } from '../store/usePairStore'
import { isPairActive } from '../lib/pairStatus'

export type ShellSection = 'pairs' | 'activity' | 'insights'

interface AppShellProps {
  section: ShellSection
  onSelectSection: (section: ShellSection) => void
  collapsed: boolean
  children: React.ReactNode
}

const SECTION_ICONS: Record<ShellSection, React.ReactNode> = {
  pairs: <Layers size={15} />,
  activity: <Clock3 size={15} />,
  insights: <BarChart3 size={15} />
}

const SECTION_LABEL_KEYS: Record<ShellSection, string> = {
  pairs: 'shell.pairs',
  activity: 'shell.activity',
  insights: 'shell.insights'
}

const SECTIONS: ShellSection[] = ['pairs', 'activity', 'insights']

export function AppShell({
  section,
  onSelectSection,
  collapsed,
  children
}: AppShellProps): React.ReactNode {
  const { t } = useTranslation()
  const pairs = usePairStore((state) => state.pairs)
  const runningCount = pairs.filter((p) => isPairActive(p.status)).length
  const attentionCount = pairs.filter(
    (p) => p.status === 'Error' || p.status === 'Awaiting Human Review'
  ).length

  return (
    <div className="flex h-full min-w-0 flex-1 overflow-hidden">
      {/* Source-list rail: translucent sidebar surface, section items with
          badge counts. Collapses to an icon strip. */}
      <nav
        aria-label={t('shell.navLabel')}
        data-testid="app-shell-rail"
        className={cn(
          'app-source-list flex shrink-0 flex-col gap-1 border-r border-border px-2 py-3 transition-[width] duration-150',
          collapsed ? 'w-12 items-center' : 'w-44'
        )}
      >
        {SECTIONS.map((key) => {
          const isActive = section === key
          const badge = key === 'pairs' ? runningCount : key === 'activity' ? null : null
          return (
            <button
              key={key}
              type="button"
              onClick={() => onSelectSection(key)}
              aria-current={isActive ? 'page' : undefined}
              data-testid={`shell-nav-${key}`}
              title={collapsed ? t(SECTION_LABEL_KEYS[key]) : undefined}
              aria-label={collapsed ? t(SECTION_LABEL_KEYS[key]) : undefined}
              className={cn(
                'relative flex w-full items-center gap-2.5 rounded-md border px-2.5 py-2 text-left text-[13px] transition-colors cursor-pointer',
                collapsed && 'justify-center px-0',
                isActive
                  ? cn('border-foreground/20 font-medium text-foreground', activeTint)
                  : cn('border-transparent text-muted-foreground hover:text-foreground', hoverTint)
              )}
            >
              {isActive && (
                <span
                  aria-hidden
                  className="absolute inset-y-1.5 left-0 w-[2px] rounded-full bg-foreground/70"
                />
              )}
              <span aria-hidden className="shrink-0">
                {SECTION_ICONS[key]}
              </span>
              {!collapsed && (
                <span className="min-w-0 flex-1 truncate">{t(SECTION_LABEL_KEYS[key])}</span>
              )}
              {!collapsed && badge !== null && badge > 0 && (
                <span
                  data-testid="shell-nav-running-badge"
                  className="shrink-0 rounded-full border border-state-running/50 bg-state-running/15 px-1.5 text-[10px] tabular-nums state-running"
                >
                  {badge}
                </span>
              )}
              {!collapsed && key === 'activity' && attentionCount > 0 && (
                <span
                  data-testid="shell-nav-attention-badge"
                  className="shrink-0 rounded-full border border-state-error/50 bg-state-error/15 px-1.5 text-[10px] tabular-nums state-error"
                >
                  {attentionCount}
                </span>
              )}
            </button>
          )
        })}
      </nav>

      <div className="flex min-w-0 flex-1 flex-col overflow-hidden">{children}</div>
    </div>
  )
}
