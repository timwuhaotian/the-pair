import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, Clipboard, Download } from 'lucide-react'
import { TimelineIterationGroup } from './TimelineIterationGroup'
import { IconButton } from './ui/IconButton'
import { TerminalDivider } from './terminal/TerminalDivider'
import type { TimelineData } from '../lib/timeline'
import { formatDuration, formatTokenCount } from '../lib/timeline'

interface TimelinePanelProps {
  timeline: TimelineData | null
  /** Rendered inside an InspectorSection; when set, skip the own header. */
  bare?: boolean
}

export function TimelineActions({ timeline }: { timeline: TimelineData }): React.ReactNode {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

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
    <span className="flex items-center gap-0.5 shrink-0">
      <IconButton aria-label={t('pair.copyTimelineMarkdown')} onClick={() => void handleCopy()}>
        {copied ? <Check size={12} className="state-done" /> : <Clipboard size={12} />}
      </IconButton>
      <IconButton aria-label={t('pair.exportTimelineHtml')} onClick={() => void handleExport()}>
        <Download size={12} />
      </IconButton>
    </span>
  )
}

function TimelineBody({ timeline }: { timeline: TimelineData | null }): React.ReactNode {
  const { t } = useTranslation()

  if (!timeline) {
    return <p className="text-[11px] text-muted-foreground-faint">{t('pair.noTimelineEvents')}</p>
  }

  const hasEvents = timeline.iterations.length > 0

  return (
    <div className="shell-card overflow-hidden">
      <div className="flex items-baseline gap-3 border-b border-border/40 px-2 py-1 text-[10px] text-muted-foreground tabular-nums">
        <span>{formatDuration(timeline.durationMs)}</span>
        <span>·</span>
        <span>{timeline.iterations.length} iter</span>
        {timeline.totalOutputTokens > 0 && (
          <>
            <span>·</span>
            <span>
              {formatTokenCount(timeline.totalOutputTokens + (timeline.totalInputTokens ?? 0))}{' '}
              {t('console.tokUnit')}
            </span>
          </>
        )}
      </div>

      <div className="max-h-[400px] overflow-y-auto scrollbar-thin p-2">
        {!hasEvents ? (
          <p className="text-[11px] text-muted-foreground-faint">{t('pair.noTimelineEvents')}</p>
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
  )
}

export function TimelinePanel({ timeline, bare = false }: TimelinePanelProps): React.JSX.Element {
  const { t } = useTranslation()
  const hasEvents = (timeline?.iterations.length ?? 0) > 0

  if (bare) return <TimelineBody timeline={timeline} />

  return (
    <div className="space-y-1">
      <div className="flex items-baseline justify-between gap-2">
        <TerminalDivider label={t('pair.timeline')} className="flex-1" />
        {hasEvents && timeline && <TimelineActions timeline={timeline} />}
      </div>
      <TimelineBody timeline={timeline} />
    </div>
  )
}
