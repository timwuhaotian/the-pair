import React from 'react'
import { motion } from 'framer-motion'
import { Terminal, FileText, Pencil, Search, Brain, type LucideProps } from 'lucide-react'
import { cn } from '../../lib/utils'
import type { CognitiveEvent } from '../../store/usePairStore'

const TOOL_ICONS: Array<[string, React.ComponentType<LucideProps>]> = [
  ['bash', Terminal],
  ['shell', Terminal],
  ['exec', Terminal],
  ['run', Terminal],
  ['readfile', FileText],
  ['read', FileText],
  ['writefile', Pencil],
  ['write', Pencil],
  ['edit', Pencil],
  ['create', Pencil],
  ['grep', Search],
  ['glob', Search],
  ['search', Search]
]

function ToolIcon({
  toolName,
  eventType,
  className
}: {
  toolName: string
  eventType: CognitiveEvent['eventType']
  className?: string
}): React.ReactNode {
  if (eventType === 'reasoning') return <Brain size={10} className={className} />
  const lower = toolName.toLowerCase()
  for (const [key, Match] of TOOL_ICONS) {
    if (lower.includes(key)) return <Match size={10} className={className} />
  }
  return <Terminal size={10} className={className} />
}

interface TerminalEventRowProps {
  event: CognitiveEvent
  /** Position in the tree — last child gets `└─`, others get `├─`. */
  isLast?: boolean
  /** Whether this is the currently-running event (gets blink + highlight). */
  isLatest?: boolean
  /** Render without motion (for finalized step-replay where there's no need for entrance anim). */
  flat?: boolean
}

const STATUS_GLYPH: Record<CognitiveEvent['status'], string> = {
  running: '⟳',
  completed: '✓',
  error: '✗'
}

export function TerminalEventRow({
  event,
  isLast = false,
  isLatest = false,
  flat = false
}: TerminalEventRowProps): React.ReactNode {
  const Wrap = flat ? 'div' : motion.div
  const wrapProps = flat
    ? {}
    : {
        layout: 'position' as const,
        initial: { opacity: 0, x: -2 },
        animate: { opacity: 1, x: 0 },
        transition: { duration: 0.12 }
      }

  const isRunning = event.status === 'running'
  const isError = event.status === 'error'
  const isCompleted = event.status === 'completed'

  return (
    <Wrap
      {...wrapProps}
      className={cn(
        'flex items-baseline gap-1 font-mono text-[11px] leading-snug',
        isError && 'state-error'
      )}
    >
      {/* tree branch */}
      <span aria-hidden className="shrink-0 select-none tabular-nums text-muted-foreground-faint">
        {isLast ? '└─' : '├─'}
      </span>
      {/* spinning loader on the latest running event — sits to the left of the description */}
      {isRunning && isLatest ? (
        <span
          aria-hidden
          className="inline-flex h-[1em] w-[1.4ch] shrink-0 items-center justify-center state-running"
        >
          <span className="tty-spin">✻</span>
        </span>
      ) : (
        <span
          aria-hidden
          className={cn(
            'inline-flex shrink-0 translate-y-[1px]',
            isRunning && 'state-running',
            isError && 'state-error',
            isCompleted && 'text-muted-foreground-faint',
            !event.status && 'text-muted-foreground-faint'
          )}
        >
          <ToolIcon toolName={event.toolName ?? ''} eventType={event.eventType} />
        </span>
      )}
      {/* description */}
      <span
        className={cn(
          'min-w-0 flex-1 truncate',
          isRunning && 'state-running',
          isCompleted && 'text-foreground/75',
          isError && 'state-error'
        )}
        title={event.description}
      >
        {event.description}
      </span>
      {/* status glyph (always rightmost) */}
      <span
        className={cn(
          'shrink-0 tabular-nums',
          isRunning && 'state-running',
          isError && 'state-error',
          isCompleted && 'state-done'
        )}
      >
        {STATUS_GLYPH[event.status]}
      </span>
    </Wrap>
  )
}
