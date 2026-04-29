import React, { useState } from 'react'
import { motion } from 'framer-motion'
import { cn, stripSystemPrompt } from '../lib/utils'
import { isAcceptanceVerdictContent, isAcceptanceRecordContent } from '../lib/acceptance'
import { isTechnicalHandoff } from '../lib/timeline'
import { formatTimestamp } from '../lib/timeline'
import { getRoleColors } from '../lib/helpers'
import { TokenChip } from './TokenChip'
import { MarkdownContent } from './MarkdownContent'
import { AcceptanceMessageBody } from './AcceptanceMessageBody'
import { Message } from '../store/usePairStore'

export function MessageCard({ msg }: { msg: Message }): React.ReactNode {
  const [isExpanded, setIsExpanded] = useState(false)
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

  // Filter out technical handoff messages (but keep human mission specs)
  if (!displayContent || displayContent === '{}' || displayContent === '[]') return null

  // Only filter handoff prompts for non-executor, non-human messages
  // Executor messages should always be displayed even if they contain handoff-like content
  if (isTechnicalHandoff(displayContent) && !isHuman && msg.from !== 'executor') return null

  return (
    <motion.div
      initial={{ opacity: 0, y: 12, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      className={cn(
        'group flex flex-col gap-3 rounded-2xl border p-5 transition-all duration-300 hover:shadow-xl hover:brightness-[1.02]',
        getRoleColors(msg.from, isHuman),
        isSystem && 'opacity-60 grayscale border-transparent bg-transparent py-2 px-0 shadow-none'
      )}
    >
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-2.5">
          {!isSystem && (
            <div
              className={cn(
                'flex items-center justify-center h-7 w-7 rounded-xl text-[11px] font-black text-white shadow-md',
                msg.from === 'mentor'
                  ? 'bg-blue-600'
                  : msg.from === 'executor'
                    ? 'bg-purple-600'
                    : 'bg-green-600'
              )}
            >
              {msg.from[0].toUpperCase()}
            </div>
          )}
          <div className="flex flex-col gap-0">
            <span
              className={cn(
                'text-[11px] font-black uppercase tracking-[0.1em]',
                msg.from === 'mentor'
                  ? 'text-blue-600'
                  : msg.from === 'executor'
                    ? 'text-purple-600'
                    : 'text-green-600'
              )}
            >
              {msg.from === 'human' ? 'MISSION SPECS' : msg.from}
            </span>
            <span
              className={cn(
                'text-[9px] font-bold tracking-tight opacity-60',
                msg.from === 'mentor'
                  ? 'text-blue-500'
                  : msg.from === 'executor'
                    ? 'text-purple-500'
                    : 'text-green-500'
              )}
            >
              {(isAcceptance ? 'acceptance' : msg.type).toUpperCase()}
            </span>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {msg.from !== 'human' && <TokenChip usage={msg.tokenUsage} compact />}
          <span className="text-[10px] font-mono text-muted-foreground/30 tabular-nums">
            {formatTimestamp(msg.timestamp)}
          </span>
        </div>
      </div>

      <div
        className={cn(
          'relative flex flex-col gap-2 overflow-hidden transition-all duration-300',
          !isExpanded && displayContent.length > 600 && 'max-h-[250px]'
        )}
      >
        <div
          className={cn(
            'break-words leading-relaxed selection:bg-primary/20 [overflow-wrap:anywhere]',
            isSystem
              ? 'text-[12px] italic text-muted-foreground'
              : 'text-[14px] text-foreground/90 font-sans'
          )}
        >
          {isAcceptance ? (
            <AcceptanceMessageBody content={displayContent} />
          ) : (
            <MarkdownContent content={displayContent} />
          )}
        </div>

        {!isExpanded && displayContent.length > 600 && (
          <div className="absolute bottom-0 left-0 right-0 h-24 bg-gradient-to-t from-background/90 to-transparent pointer-events-none" />
        )}
      </div>

      {displayContent.length > 600 && (
        <button
          onClick={() => setIsExpanded(!isExpanded)}
          className="w-fit text-[10px] font-bold uppercase tracking-widest text-primary hover:underline transition-all cursor-pointer"
        >
          {isExpanded ? 'Collapse' : 'Expand full report'}
        </button>
      )}
    </motion.div>
  )
}
