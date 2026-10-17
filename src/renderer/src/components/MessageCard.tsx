import React, { memo, useMemo, useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { ChevronDown, ChevronRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { isAcceptanceVerdictContent, isAcceptanceRecordContent } from '../lib/acceptance'
import { isTechnicalHandoff } from '../lib/timeline'
import { displayContentForMessage } from '../lib/searchableCard'
import { MarkdownContent } from './MarkdownContent'
import { AcceptanceMessageBody } from './AcceptanceMessageBody'
import { TerminalBlock } from './terminal/TerminalBlock'
import { TerminalEventRow } from './terminal/TerminalEventRow'
import { SystemBanner } from './SystemBanner'
import { TokenChip } from './TokenChip'
import type { Message } from '../store/usePairStore'

const CONTENT_COLLAPSE_THRESHOLD = 1200

function formatDuration(ms: number | undefined): string | undefined {
  if (!ms || ms <= 0) return undefined
  const seconds = Math.round(ms / 1000)
  if (seconds < 60) return `${seconds}s`
  const mins = Math.floor(seconds / 60)
  const rem = seconds % 60
  return rem === 0 ? `${mins}m` : `${mins}m ${rem}s`
}

function typeLabel(type: Message['type']): string {
  switch (type) {
    case 'plan':
      return 'plan'
    case 'result':
      return 'result'
    case 'acceptance':
      return 'review'
    case 'feedback':
      return 'feedback'
    case 'question':
      return 'question'
    case 'progress':
      return 'progress'
    default:
      return type
  }
}

export const MessageCard = memo(function MessageCard({
  msg,
  searchQuery,
  isActiveMatch = false
}: {
  msg: Message
  searchQuery?: string
  isActiveMatch?: boolean
}): React.ReactNode {
  const { t } = useTranslation()
  // Single expansion state covers both cognitive steps and long-body collapse.
  // When a message has both kinds of hidden content, users used to face two
  // separate toggles in different parts of the card — combining them removes
  // that choice-paralysis without losing functionality.
  const [detailsExpanded, setDetailsExpanded] = useState(false)

  const isSystem = msg.type === 'handoff'
  const isHuman = msg.from === 'human'

  const displayContent = displayContentForMessage(msg)

  const isAcceptance =
    msg.type === 'acceptance' ||
    isAcceptanceVerdictContent(displayContent) ||
    isAcceptanceRecordContent(displayContent)

  const cognitiveEvents = useMemo(
    () =>
      (msg.cognitiveEvents ?? [])
        .filter((e) => e.eventType !== 'error' || e.description.trim().length > 0)
        .sort((a, b) => a.timestamp - b.timestamp),
    [msg.cognitiveEvents]
  )

  // Skip empty / dropped messages
  if (!displayContent || displayContent === '{}' || displayContent === '[]') return null
  if (isTechnicalHandoff(displayContent) && !isHuman && msg.from !== 'executor') return null
  if (isSystem) return null

  // Human messages become a centered system banner
  if (isHuman) {
    const variant: 'mission' | 'human-feedback' =
      msg.type === 'feedback' && msg.iteration > 0 ? 'human-feedback' : 'mission'
    return <SystemBanner variant={variant} content={displayContent} timestamp={msg.timestamp} />
  }

  const role = msg.from as 'mentor' | 'executor'
  const hasSteps = cognitiveEvents.length > 0
  const duration = formatDuration(
    msg.startedAt && msg.finalizedAt ? msg.finalizedAt - msg.startedAt : undefined
  )
  const showCollapse = displayContent.length > CONTENT_COLLAPSE_THRESHOLD
  const hasExpandable = hasSteps || showCollapse
  // The active search match must be visible: a match below the collapse fold
  // would otherwise scroll into view still clipped. Lift only the 280px body
  // clip while active and leave the toggle and the steps panel to
  // detailsExpanded, so the control never fights the search override.
  const bodyClipped = showCollapse && !detailsExpanded && !isActiveMatch

  // Label for the unified toggle reflects exactly what's hidden behind it.
  const expandLabel = ((): string => {
    if (detailsExpanded) return t('console.hideDetails')
    if (hasSteps && showCollapse) {
      return t('console.showDetailsBoth', { count: cognitiveEvents.length })
    }
    if (hasSteps) {
      return t('console.showStepsOnly', { count: cognitiveEvents.length })
    }
    return t('console.showReportOnly')
  })()

  const typeTag = (
    <span
      className={cn(
        'rounded-sm border px-1 py-0 text-[9px] uppercase tracking-[0.14em]',
        isAcceptance
          ? 'border-state-running/40 bg-state-running/15 state-running'
          : role === 'mentor'
            ? 'border-role-mentor bg-role-mentor role-mentor'
            : 'border-role-executor bg-role-executor role-executor'
      )}
    >
      {isAcceptance ? 'acceptance' : typeLabel(msg.type)}
    </span>
  )

  return (
    <TerminalBlock
      role={role}
      state="done"
      id={`console-msg-${msg.id}`}
      timestamp={msg.startedAt ?? msg.timestamp}
      badges={typeTag}
      accentBorder
      className={cn(
        'rounded-sm transition-shadow',
        isActiveMatch && 'outline outline-1 outline-state-running/60'
      )}
      meta={
        <span className="flex items-center gap-2 text-[10px] uppercase tracking-[0.14em] text-muted-foreground">
          {duration && <span className="tabular-nums">{duration}</span>}
          {msg.tokenUsage && <TokenChip usage={msg.tokenUsage} compact />}
        </span>
      }
    >
      {/* Unified expand control — covers cognitive steps and/or long-body collapse.
          Hidden on the active match when it would have no effect (no steps and
          the body clip is already lifted). */}
      {hasExpandable && (hasSteps || !isActiveMatch) && (
        <div className="mb-1.5">
          <button
            onClick={() => setDetailsExpanded((v) => !v)}
            className={cn(
              'group flex items-baseline gap-1.5 font-mono text-[10px] uppercase tracking-[0.14em]',
              'text-muted-foreground-faint hover:text-foreground/80 transition-colors'
            )}
            aria-expanded={detailsExpanded}
          >
            <span className="text-muted-foreground-faint">{detailsExpanded ? '└─' : '├─'}</span>
            {detailsExpanded ? (
              <ChevronDown size={10} className="text-muted-foreground-faint translate-y-px" />
            ) : (
              <ChevronRight size={10} className="text-muted-foreground-faint translate-y-px" />
            )}
            <span className="text-foreground/70 normal-case tracking-normal">{expandLabel}</span>
          </button>
          {hasSteps && (
            <AnimatePresence initial={false}>
              {detailsExpanded && (
                <motion.div
                  initial={{ height: 0, opacity: 0 }}
                  animate={{ height: 'auto', opacity: 1 }}
                  exit={{ height: 0, opacity: 0 }}
                  transition={{ duration: 0.16 }}
                  className="overflow-hidden"
                >
                  <div className="mt-1 space-y-px">
                    {cognitiveEvents.map((event, idx) => (
                      <TerminalEventRow
                        key={event.id}
                        event={event}
                        isLast={idx === cognitiveEvents.length - 1}
                        flat
                      />
                    ))}
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          )}
        </div>
      )}

      {/* Prose */}
      <div
        className={cn(
          'text-[12px] leading-relaxed',
          bodyClipped && 'max-h-[280px] overflow-hidden relative'
        )}
      >
        {isAcceptance ? (
          <AcceptanceMessageBody content={displayContent} />
        ) : (
          <MarkdownContent content={displayContent} searchQuery={searchQuery} />
        )}
        {bodyClipped && (
          <button
            onClick={() => setDetailsExpanded(true)}
            className="absolute bottom-0 left-0 right-0 h-12 bg-gradient-to-t from-background to-transparent cursor-pointer flex items-end justify-center pb-1 text-[10px] uppercase tracking-[0.14em] text-muted-foreground hover:text-foreground/80 transition-colors"
            aria-label={t('console.expand')}
          >
            <span className={role === 'mentor' ? 'role-mentor' : 'role-executor'}>
              {t('console.showReportOnly')}
            </span>
          </button>
        )}
      </div>
    </TerminalBlock>
  )
})
