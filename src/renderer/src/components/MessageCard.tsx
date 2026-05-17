import React, { useMemo, useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { ChevronDown, ChevronRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn, stripSystemPrompt } from '../lib/utils'
import { isAcceptanceVerdictContent, isAcceptanceRecordContent } from '../lib/acceptance'
import { isTechnicalHandoff } from '../lib/timeline'
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

export function MessageCard({ msg }: { msg: Message }): React.ReactNode {
  const { t } = useTranslation()
  const [stepsExpanded, setStepsExpanded] = useState(false)
  const [contentExpanded, setContentExpanded] = useState(false)

  const isSystem = msg.type === 'handoff'
  const isHuman = msg.from === 'human'

  const displayContent =
    isHuman || isTechnicalHandoff(msg.content)
      ? stripSystemPrompt(msg.content.trim())
      : msg.content.trim()

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
      timestamp={msg.startedAt ?? msg.timestamp}
      badges={typeTag}
      meta={
        <span className="flex items-center gap-2 text-[10px] uppercase tracking-[0.14em] text-muted-foreground">
          {duration && <span className="tabular-nums">{duration}</span>}
          {msg.tokenUsage && <TokenChip usage={msg.tokenUsage} compact />}
        </span>
      }
    >
      {/* Collapsible step replay */}
      {hasSteps && (
        <div className="mb-1.5">
          <button
            onClick={() => setStepsExpanded((v) => !v)}
            className={cn(
              'group flex items-baseline gap-1.5 font-mono text-[10px] uppercase tracking-[0.14em]',
              'text-muted-foreground-faint hover:text-foreground/80 transition-colors'
            )}
          >
            <span className="text-muted-foreground-faint">{stepsExpanded ? '└─' : '├─'}</span>
            {stepsExpanded ? (
              <ChevronDown size={10} className="text-muted-foreground-faint translate-y-px" />
            ) : (
              <ChevronRight size={10} className="text-muted-foreground-faint translate-y-px" />
            )}
            <span className="text-foreground/70">
              {cognitiveEvents.length} {cognitiveEvents.length === 1 ? 'step' : 'steps'}
            </span>
            <span className="text-muted-foreground-faint normal-case tracking-normal">
              · {stepsExpanded ? t('console.collapse').toLowerCase() : 'expand'}
            </span>
          </button>
          <AnimatePresence initial={false}>
            {stepsExpanded && (
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
        </div>
      )}

      {/* Prose */}
      <div
        className={cn(
          'text-[12px] leading-relaxed',
          !contentExpanded && showCollapse && 'max-h-[280px] overflow-hidden relative'
        )}
      >
        {isAcceptance ? (
          <AcceptanceMessageBody content={displayContent} />
        ) : (
          <MarkdownContent content={displayContent} />
        )}
        {!contentExpanded && showCollapse && (
          <div className="pointer-events-none absolute bottom-0 left-0 right-0 h-12 bg-gradient-to-t from-background to-transparent" />
        )}
      </div>
      {showCollapse && (
        <button
          onClick={() => setContentExpanded((v) => !v)}
          className={cn(
            'mt-1 font-mono text-[10px] uppercase tracking-[0.14em] hover:underline',
            role === 'mentor' ? 'role-mentor' : 'role-executor'
          )}
        >
          {contentExpanded
            ? `└─ ${t('console.collapse').toLowerCase()}`
            : `└─ ${t('console.expand').toLowerCase()}`}
        </button>
      )}
    </TerminalBlock>
  )
}
