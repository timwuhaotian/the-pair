import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { isTauri, tauriApi } from '../lib/tauri-api'
import { cn } from '../lib/utils'
import { sectionLabel } from '../lib/ui'
import type { InsightsSummary } from '../types'

/** Hide the panel entirely until enough real runs have accumulated. */
const COLD_START_MIN_RUNS = 5

const MAX_COMBO_ROWS = 5

function formatTokens(tokens: number): string {
  if (tokens >= 1000) {
    return `${(tokens / 1000).toFixed(1)}k`
  }
  return `${Math.round(tokens)}`
}

interface InsightsPanelProps {
  className?: string
}

export function InsightsPanel({ className }: InsightsPanelProps): React.ReactNode {
  const { t } = useTranslation()
  const [summary, setSummary] = useState<InsightsSummary | null>(null)

  useEffect(() => {
    if (!isTauri) return
    let cancelled = false

    const load = async (): Promise<void> => {
      try {
        const result = await tauriApi.insights.getSummary()
        if (!cancelled) setSummary(result)
      } catch (error) {
        console.warn('[InsightsPanel] Failed to load insights summary:', error)
      }
    }

    void load()
    const interval = setInterval(() => void load(), 30000)
    return () => {
      cancelled = true
      clearInterval(interval)
    }
  }, [])

  // Cold start: don't show an empty leaderboard, but never render a blank
  // section either — tell the user what unlocks it.
  if (!summary || summary.totalRuns < COLD_START_MIN_RUNS) {
    return (
      <section
        data-testid="insights-cold-start"
        className={cn('flex flex-col gap-2 px-1 py-1', className)}
      >
        <h2 className={sectionLabel}>{t('dashboard.crossRun.title')}</h2>
        <p className="text-[13px] text-muted-foreground">
          {t('dashboard.crossRun.coldStart', {
            min: COLD_START_MIN_RUNS,
            count: summary?.totalRuns ?? 0
          })}
        </p>
      </section>
    )
  }

  return (
    <section
      data-testid="insights-panel"
      className={cn('flex flex-col gap-2 px-1 py-1', className)}
    >
      <h2 className={sectionLabel}>{t('dashboard.crossRun.title')}</h2>

      <div className="flex items-baseline gap-2">
        <span className="text-foreground tabular-nums text-[22px] font-semibold leading-none">
          {Math.round(summary.successRate * 100)}%
        </span>
        <span className="text-[13px] text-muted-foreground">
          {t('dashboard.crossRun.successRate', {
            successes: summary.successfulRuns,
            total: summary.totalRuns
          })}
        </span>
      </div>

      {summary.combos.length > 0 && (
        <table className="w-full text-[12px] leading-relaxed">
          <thead>
            <tr className="text-left text-[11px] uppercase tracking-[0.14em] text-muted-foreground-faint">
              <th className="py-1 pr-1 font-medium">{t('dashboard.crossRun.combo')}</th>
              <th className="py-1 pr-1 text-right font-medium">{t('dashboard.crossRun.runs')}</th>
              <th className="py-1 pr-1 text-right font-medium">
                {t('dashboard.crossRun.success')}
              </th>
              <th className="py-1 pr-1 text-right font-medium">
                {t('dashboard.crossRun.avgIter')}
              </th>
              <th className="py-1 text-right font-medium">{t('dashboard.crossRun.avgTokens')}</th>
            </tr>
          </thead>
          <tbody>
            {summary.combos.slice(0, MAX_COMBO_ROWS).map((combo) => (
              <tr
                key={`${combo.mentorModel}|${combo.executorModel}`}
                className="border-t border-border/60 text-foreground/85"
              >
                <td className="max-w-0 truncate py-1.5 pr-1 font-mono text-[12px]">
                  <span className="role-mentor">{combo.mentorModel}</span>
                  <span className="text-muted-foreground-faint"> + </span>
                  <span className="role-executor">{combo.executorModel}</span>
                </td>
                <td className="py-1.5 pr-1 text-right tabular-nums">{combo.runs}</td>
                <td className="py-1.5 pr-1 text-right tabular-nums">
                  {Math.round(combo.successRate * 100)}%
                </td>
                <td className="py-1.5 pr-1 text-right tabular-nums">
                  {combo.avgIterations.toFixed(1)}
                </td>
                <td className="py-1.5 text-right tabular-nums">{formatTokens(combo.avgTokens)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  )
}
