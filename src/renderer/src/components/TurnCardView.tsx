import React, { useMemo, useState } from 'react'
import { AnimatePresence } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { isAcceptanceVerdictContent, isAcceptanceRecordContent } from '../lib/acceptance'
import { MarkdownContent } from './MarkdownContent'
import { AcceptanceMessageBody } from './AcceptanceMessageBody'
import { TerminalBlock } from './terminal/TerminalBlock'
import { TerminalEventRow } from './terminal/TerminalEventRow'
import { TurnCard } from '../store/usePairStore'
import { TokenChip } from './TokenChip'

const MAX_INLINE_EVENTS = 8

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds}s`
  const mins = Math.floor(seconds / 60)
  const rem = seconds % 60
  return rem === 0 ? `${mins}m` : `${mins}m ${rem}s`
}

export function TurnCardView({ card }: { card: TurnCard }): React.ReactNode {
  const { t } = useTranslation()
  const [showEarlierSteps, setShowEarlierSteps] = useState(false)

  const cognitiveEvents = useMemo(
    () =>
      (card.cognitiveEvents ?? [])
        .filter((e) => e.eventType !== 'error' || e.description.trim().length > 0)
        .sort((a, b) => a.timestamp - b.timestamp),
    [card.cognitiveEvents]
  )

  const visibleEvents = useMemo(() => {
    if (cognitiveEvents.length <= MAX_INLINE_EVENTS) return cognitiveEvents
    if (showEarlierSteps) return cognitiveEvents
    return cognitiveEvents.slice(-MAX_INLINE_EVENTS)
  }, [cognitiveEvents, showEarlierSteps])

  const hiddenCount = cognitiveEvents.length - visibleEvents.length
  const currentAction = (card.content || card.activity.detail || card.activity.label || '').trim()

  const isAcceptance = useMemo(() => {
    if (card.role !== 'mentor') return false
    return isAcceptanceVerdictContent(currentAction) || isAcceptanceRecordContent(currentAction)
  }, [card.role, currentAction])

  const phase = card.activity.phase
  const isStalled = phase === 'stalled'
  const isErrored = phase === 'error'
  const isRunning = phase === 'thinking' || phase === 'using_tools' || phase === 'responding'

  // Keep the role glyph static — the running animator lives on the latest event / body line
  const state = isErrored ? 'error' : isStalled ? 'paused' : 'done'

  // header meta line — e.g., "thinking · 12s · 3 steps"
  const elapsedSec =
    card.activity.startedAt && card.activity.updatedAt
      ? Math.floor((card.activity.updatedAt - card.activity.startedAt) / 1000)
      : 0
  const phaseLabel =
    phase === 'thinking'
      ? t('activity.thinking').toLowerCase()
      : phase === 'using_tools'
        ? (card.activity.detail?.trim() || t('activity.toolCall')).toLowerCase()
        : phase === 'responding'
          ? t('activity.output').toLowerCase()
          : phase

  const metaParts: string[] = []
  if (isRunning && phaseLabel) metaParts.push(phaseLabel)
  if (isRunning && elapsedSec > 0) metaParts.push(formatDuration(elapsedSec))
  if (cognitiveEvents.length > 0)
    metaParts.push(`${cognitiveEvents.length} ${cognitiveEvents.length === 1 ? 'step' : 'steps'}`)

  const badges = isStalled ? (
    <span className="rounded-sm border border-state-error bg-state-error/15 px-1.5 py-0 text-[9px] font-bold uppercase tracking-[0.16em] state-error tty-blink">
      stalled
    </span>
  ) : null

  return (
    <TerminalBlock
      role={card.role}
      state={state}
      timestamp={card.startedAt}
      badges={badges}
      meta={
        <span className="flex items-center gap-2 text-[10px] uppercase tracking-[0.14em] text-muted-foreground">
          {metaParts.length > 0 && <span>{metaParts.join(' · ')}</span>}
          {card.tokenUsage && <TokenChip usage={card.tokenUsage} isLive compact />}
        </span>
      }
    >
      {/* Tree of cognitive events */}
      {visibleEvents.length > 0 && (
        <div className="mb-1.5 space-y-px">
          {(hiddenCount > 0 || showEarlierSteps) && cognitiveEvents.length > MAX_INLINE_EVENTS && (
            <button
              type="button"
              onClick={() => setShowEarlierSteps((v) => !v)}
              className="font-mono text-[10px] text-muted-foreground-faint hover:text-foreground/80 uppercase tracking-[0.14em] transition-colors cursor-pointer"
            >
              {showEarlierSteps
                ? `· ${t('console.earlierStepsHide')} ·`
                : `· ${t('console.earlierStepsButton', { count: hiddenCount })} ·`}
            </button>
          )}
          <AnimatePresence initial={false}>
            {visibleEvents.map((event, idx) => (
              <TerminalEventRow
                key={event.id}
                event={event}
                isLast={idx === visibleEvents.length - 1}
                isLatest={idx === visibleEvents.length - 1}
              />
            ))}
          </AnimatePresence>
        </div>
      )}

      {/* Prose body */}
      {currentAction.length > 0 && (
        <div
          className={cn(
            'flex items-baseline gap-1.5 text-[12px] leading-relaxed',
            isStalled && 'state-error'
          )}
        >
          {/* When there are no cognitive event rows yet, the animator lives next to the body line. */}
          {isRunning && visibleEvents.length === 0 && (
            <span
              aria-hidden
              className={cn(
                'inline-flex h-[1em] w-[1.4ch] shrink-0 items-center justify-center',
                card.role === 'mentor' ? 'role-mentor' : 'role-executor'
              )}
            >
              <span className="tty-spin">✻</span>
            </span>
          )}
          <div className="min-w-0 flex-1">
            {isAcceptance ? (
              <AcceptanceMessageBody content={currentAction} />
            ) : (
              <MarkdownContent content={currentAction} />
            )}
          </div>
        </div>
      )}

      {currentAction.length === 0 && visibleEvents.length === 0 && (
        <div className="flex items-baseline gap-1.5 text-[12px] text-muted-foreground-faint">
          <span
            aria-hidden
            className="inline-flex h-[1em] w-[1.4ch] items-center justify-center state-running"
          >
            <span className="tty-spin">✻</span>
          </span>
          {t('common.thinking')}
        </div>
      )}
    </TerminalBlock>
  )
}
