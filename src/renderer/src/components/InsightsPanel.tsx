import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { isTauri, tauriApi } from '../lib/tauri-api'
import { cn } from '../lib/utils'
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

  // Cold start: stay hidden until the user has enough run history.
  if (!summary || summary.totalRuns < COLD_START_MIN_RUNS) {
    return null
  }

  return (
    <section
      data-testid="insights-panel"
      className={cn(
        'flex flex-col gap-2 border border-border bg-background/40 px-3 py-2 font-mono',
        className
      )}
    >
      <h2 className="text-[10px] uppercase tracking-[0.18em] text-foreground/85">
        <span className="text-primary">──</span> {t('dashboard.crossRun.title')}{' '}
        <span className="text-primary">──</span>
      </h2>

      <div className="flex items-baseline gap-2 text-[11px]">
        <span className="text-foreground/90 tabular-nums text-[15px] font-bold leading-none">
          {Math.round(summary.successRate * 100)}%
        </span>
        <span className="text-muted-foreground">
          {t('dashboard.crossRun.successRate', {
            successes: summary.successfulRuns,
            total: summary.totalRuns
          })}
        </span>
      </div>

      {summary.combos.length > 0 && (
        <table className="w-full text-[10px] leading-relaxed">
          <thead>
            <tr className="text-left uppercase tracking-[0.12em] text-muted-foreground-faint">
              <th className="py-1 pr-1 font-normal">{t('dashboard.crossRun.combo')}</th>
              <th className="py-1 pr-1 text-right font-normal">{t('dashboard.crossRun.runs')}</th>
              <th className="py-1 pr-1 text-right font-normal">
                {t('dashboard.crossRun.success')}
              </th>
              <th className="py-1 pr-1 text-right font-normal">
                {t('dashboard.crossRun.avgIter')}
              </th>
              <th className="py-1 text-right font-normal">{t('dashboard.crossRun.avgTokens')}</th>
            </tr>
          </thead>
          <tbody>
            {summary.combos.slice(0, MAX_COMBO_ROWS).map((combo) => (
              <tr
                key={`${combo.mentorModel}|${combo.executorModel}`}
                className="border-t border-border/60 text-foreground/85"
              >
                <td className="max-w-0 truncate py-1 pr-1">
                  <span className="role-mentor">{combo.mentorModel}</span>
                  <span className="text-muted-foreground-faint"> + </span>
                  <span className="role-executor">{combo.executorModel}</span>
                </td>
                <td className="py-1 pr-1 text-right tabular-nums">{combo.runs}</td>
                <td className="py-1 pr-1 text-right tabular-nums">
                  {Math.round(combo.successRate * 100)}%
                </td>
                <td className="py-1 pr-1 text-right tabular-nums">
                  {combo.avgIterations.toFixed(1)}
                </td>
                <td className="py-1 text-right tabular-nums">{formatTokens(combo.avgTokens)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  )
}
