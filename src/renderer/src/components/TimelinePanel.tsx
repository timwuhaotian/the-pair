import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, Clipboard, Download } from 'lucide-react'
import { TimelineIterationGroup } from './TimelineIterationGroup'
import { TerminalDivider } from './terminal/TerminalDivider'
import type { TimelineData } from '../lib/timeline'
import { formatDuration, formatTokenCount } from '../lib/timeline'

interface TimelinePanelProps {
  timeline: TimelineData | null
}

export function TimelinePanel({ timeline }: TimelinePanelProps): React.JSX.Element {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  if (!timeline) {
    return (
      <div className="space-y-1 font-mono">
        <TerminalDivider label={t('pair.timeline')} />
        <p className="text-[10px] text-muted-foreground-faint pl-2">
          — {t('pair.noTimelineEvents')} —
        </p>
      </div>
    )
  }

  const hasEvents = timeline.iterations.length > 0

  const handleCopy = async (): Promise<void> => {
    const { copyMarkdownReport } = await import('../lib/reportExport')
    const ok = await copyMarkdownReport(timeline)
    if (ok) {
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    }
  }

  const handleExport = async (): Promise<void> => {
    try {
      const { exportAsHtml } = await import('../lib/reportExport')
      await exportAsHtml(timeline)
    } catch (err) {
      console.error('Failed to export timeline report:', err)
    }
  }

  return (
    <div className="font-mono space-y-1">
      <div className="flex items-baseline justify-between gap-2">
        <TerminalDivider label={t('pair.timeline')} className="flex-1" />
        {hasEvents && (
          <span className="flex items-center gap-1 shrink-0">
            <button
              type="button"
              onClick={handleCopy}
              title="copy as markdown"
              className="p-1 rounded-sm text-muted-foreground hover:text-foreground hover:bg-foreground/[0.06] transition-colors cursor-pointer"
            >
              {copied ? <Check size={10} className="state-done" /> : <Clipboard size={10} />}
            </button>
            <button
              type="button"
              onClick={handleExport}
              title="export as html"
              className="p-1 rounded-sm text-muted-foreground hover:text-foreground hover:bg-foreground/[0.06] transition-colors cursor-pointer"
            >
              <Download size={10} />
            </button>
          </span>
        )}
      </div>

      <div className="border border-border bg-background/40 rounded-sm overflow-hidden">
        <div className="flex items-baseline gap-3 border-b border-border/40 px-2 py-1 text-[10px] text-muted-foreground tabular-nums">
          <span>{formatDuration(timeline.durationMs)}</span>
          <span>·</span>
          <span>{timeline.iterations.length} iter</span>
          {timeline.totalOutputTokens > 0 && (
            <>
              <span>·</span>
              <span>
                {formatTokenCount(timeline.totalOutputTokens + (timeline.totalInputTokens ?? 0))}{' '}
                tok
              </span>
            </>
          )}
        </div>

        <div className="max-h-[400px] overflow-y-auto scrollbar-thin p-2">
          {!hasEvents ? (
            <p className="text-[10px] text-muted-foreground-faint">
              — {t('pair.noTimelineEvents')} —
            </p>
          ) : (
            timeline.iterations.map((group, idx) => (
              <TimelineIterationGroup
                key={group.iteration}
                group={group}
                isLast={idx === timeline.iterations.length - 1}
              />
            ))
          )}
        </div>
      </div>
    </div>
  )
}
