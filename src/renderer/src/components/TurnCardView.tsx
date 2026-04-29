import React, { useEffect, useMemo } from 'react'
import { motion } from 'framer-motion'
import { Zap } from 'lucide-react'
import { cn } from '../lib/utils'
import { usePrevious } from '../lib/usePrevious'
import { turnCardFinalize } from '../lib/animations'
import { isAcceptanceVerdictContent, isAcceptanceRecordContent } from '../lib/acceptance'
import { TokenChip } from './TokenChip'
import { MarkdownContent } from './MarkdownContent'
import { AcceptanceMessageBody } from './AcceptanceMessageBody'
import { TurnCard } from '../store/usePairStore'
import { ActivityIndicator } from './ActivityIndicator'
import { IntentChip } from './IntentChip'
import { ToolCallSteps } from './ToolCallSteps'

export function TurnCardView({ card }: { card: TurnCard }): React.ReactNode {
  const prevCardId = usePrevious(card.id)
  const cognitiveEvents = card.cognitiveEvents ?? []

  const animState = card.state === 'final' && card.finalizedAt ? 'final' : 'live'

  useEffect(() => {
    // reset handled by key change in parent wrapper
  }, [prevCardId, card.id])

  const isMentor = card.role === 'mentor'
  const accent = isMentor ? 'text-blue-500' : 'text-purple-500'
  const borderAccent = isMentor ? 'border-blue-400/30' : 'border-purple-400/30'
  const bg = isMentor ? 'bg-blue-500/6' : 'bg-purple-500/6'
  const currentAction = (
    card.content ||
    card.activity.detail ||
    card.activity.label ||
    'Working...'
  ).trim()

  const isAcceptance = useMemo(() => {
    if (card.role !== 'mentor' || card.state !== 'final') return false
    return isAcceptanceVerdictContent(currentAction) || isAcceptanceRecordContent(currentAction)
  }, [card.role, card.state, currentAction])

  return (
    <motion.div
      initial="live"
      animate={animState}
      variants={turnCardFinalize}
      layout
      className={cn(
        'relative overflow-hidden rounded-2xl border p-5 shadow-lg transition-colors duration-300',
        borderAccent,
        animState === 'final' && isMentor && 'border-blue-400/40',
        animState === 'final' && !isMentor && 'border-purple-400/40',
        card.activity.phase === 'stalled' && 'border-red-500/50 shadow-red-500/10',
        bg,
        'metal-sheen-surface'
      )}
    >
      <div className="mb-3 flex items-center gap-2 flex-wrap">
        <Zap size={14} className={cn(accent, 'drop-shadow-sm')} fill="currentColor" />
        <span className={cn('text-[10px] font-black uppercase tracking-[0.16em]', accent)}>
          {card.role.toUpperCase()}
        </span>
        <span className="text-[9px] font-mono uppercase tracking-[0.18em] text-muted-foreground/70 transition-all duration-300">
          {card.state === 'live' ? 'working' : 'result'}
        </span>
        {card.state === 'live' && <ActivityIndicator activity={card.activity} />}
        {card.state === 'live' && cognitiveEvents.length > 0 && (
          <IntentChip events={cognitiveEvents} role={card.role} />
        )}
        <TokenChip usage={card.tokenUsage} isLive={card.state === 'live'} className="ml-auto" />
      </div>
      <div
        className={cn(
          'text-sm leading-relaxed [overflow-wrap:anywhere] transition-colors duration-300',
          card.state === 'live' ? 'text-foreground/90' : 'text-foreground'
        )}
      >
        {isAcceptance ? (
          <AcceptanceMessageBody content={currentAction} />
        ) : (
          <MarkdownContent content={currentAction} />
        )}
      </div>
      {card.state === 'live' && cognitiveEvents.length > 0 && (
        <ToolCallSteps events={cognitiveEvents} />
      )}
    </motion.div>
  )
}
