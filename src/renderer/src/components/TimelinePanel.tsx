import React, { useState } from 'react'
import { Check, Clipboard, Clock, Download } from 'lucide-react'
import { GlassButton } from './ui/GlassButton'
import { TimelineIterationGroup } from './TimelineIterationGroup'
import type { TimelineData } from '../lib/timeline'
import { formatDuration, formatTokenCount } from '../lib/timeline'

interface TimelinePanelProps {
  timeline: TimelineData | null
}

export function TimelinePanel({ timeline }: TimelinePanelProps): React.JSX.Element {
  const [copied, setCopied] = useState(false)

  if (!timeline) {
    return (
      <div>
        <h3 className="mb-3 flex items-center gap-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          <Clock size={12} />
          Timeline
        </h3>
        <div className="glass-card rounded-2xl p-4">
          <p className="text-[11px] text-muted-foreground/60">No timeline events yet.</p>
        </div>
      </div>
    )
  }

  const hasEvents = timeline.iterations.length > 0

  const handleCopy = async () => {
    const { copyMarkdownReport } = await import('../lib/reportExport')
    const ok = await copyMarkdownReport(timeline)
    if (ok) {
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    }
  }

  const handleExport = async () => {
    try {
      const { exportAsHtml } = await import('../lib/reportExport')
      await exportAsHtml(timeline)
    } catch (err) {
      console.error('Failed to export timeline report:', err)
    }
  }

  return (
    <div>
      <h3 className="mb-3 flex items-center justify-between gap-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
        <span className="flex items-center gap-2">
          <Clock size={12} />
          Timeline
        </span>
        {hasEvents && (
          <span className="flex items-center gap-1">
            <GlassButton
              variant="ghost"
              size="sm"
              onClick={handleCopy}
              icon={copied ? <Check size={9} /> : <Clipboard size={9} />}
              className="h-6 w-6 min-w-0 p-0 px-1.5 [&]:text-[9px]"
              title="Copy as Markdown"
            >
              {' '}
            </GlassButton>
            <GlassButton
              variant="ghost"
              size="sm"
              onClick={handleExport}
              icon={<Download size={9} />}
              className="h-6 w-6 min-w-0 p-0 px-1.5 [&]:text-[9px]"
              title="Export as HTML"
            >
              {' '}
            </GlassButton>
          </span>
        )}
      </h3>

      <div className="glass-card rounded-2xl overflow-hidden">
        {/* Summary stats bar */}
        <div className="flex items-center gap-3 border-b border-border/30 px-3 py-2">
          <span className="text-[9px] font-mono text-muted-foreground/50">
            {formatDuration(timeline.durationMs)}
          </span>
          <span className="text-[9px] font-mono text-muted-foreground/50">
            {timeline.iterations.length} iter
          </span>
          {timeline.totalOutputTokens > 0 && (
            <span className="text-[9px] font-mono text-muted-foreground/50">
              {formatTokenCount(timeline.totalOutputTokens)} tok
            </span>
          )}
        </div>

        <div className="max-h-[400px] overflow-y-auto scrollbar-thin p-3">
          {!hasEvents ? (
            <p className="text-[11px] text-muted-foreground/60">No timeline events yet.</p>
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
