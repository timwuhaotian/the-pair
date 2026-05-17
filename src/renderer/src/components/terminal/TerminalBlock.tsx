import React from 'react'
import { motion } from 'framer-motion'
import { cn } from '../../lib/utils'

export type TerminalRole = 'mentor' | 'executor' | 'human' | 'system'
export type TerminalState = 'running' | 'done' | 'error' | 'paused' | 'pending'

interface TerminalBlockProps {
  role: TerminalRole
  state?: TerminalState
  timestamp?: number
  /** Optional override of the leading glyph (defaults computed from state). */
  glyph?: string
  /** Label shown after the prefix (uppercase). Defaults to role.toUpperCase(). */
  label?: string
  /** Status pills aligned next to the label. */
  badges?: React.ReactNode
  /** Right-aligned meta (token count, duration, etc.). */
  meta?: React.ReactNode
  /** Body content — placed below the prefix line, indented under the glyph. */
  children?: React.ReactNode
  /** Show a blinking caret at end of body. */
  caret?: boolean
  className?: string
  /** No-animation render (useful inside virtualized lists). */
  flat?: boolean
}

function formatClock(ts: number): string {
  const d = new Date(ts)
  return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}:${d.getSeconds().toString().padStart(2, '0')}`
}

const ROLE_TEXT_CLASS: Record<TerminalRole, string> = {
  mentor: 'role-mentor',
  executor: 'role-executor',
  human: 'role-human',
  system: 'text-muted-foreground'
}

const STATE_GLYPH: Record<TerminalState, string> = {
  running: '✻',
  done: '●',
  error: '✗',
  paused: '◌',
  pending: '○'
}

const STATE_TEXT_CLASS: Record<TerminalState, string> = {
  running: 'state-running',
  done: '',
  error: 'state-error',
  paused: 'text-muted-foreground',
  pending: 'text-muted-foreground-faint'
}

export function TerminalBlock({
  role,
  state = 'done',
  timestamp,
  glyph,
  label,
  badges,
  meta,
  children,
  caret = false,
  className,
  flat = false
}: TerminalBlockProps): React.ReactNode {
  const Wrap = flat ? 'div' : motion.div
  const wrapProps = flat
    ? {}
    : {
        layout: 'position' as const,
        initial: { opacity: 0, y: 4 },
        animate: { opacity: 1, y: 0 },
        transition: { duration: 0.14 }
      }

  const effectiveGlyph = glyph ?? STATE_GLYPH[state]
  const glyphClass =
    state === 'running' ? ROLE_TEXT_CLASS[role] : STATE_TEXT_CLASS[state] || ROLE_TEXT_CLASS[role]
  const labelText = label ?? role.toUpperCase()

  return (
    <Wrap {...wrapProps} className={cn('font-mono text-[12px] leading-relaxed', className)}>
      {/* PREFIX LINE */}
      <div className="flex items-baseline gap-2 whitespace-nowrap">
        {state === 'running' ? (
          <span
            aria-hidden
            className={cn(
              'relative inline-flex h-[1em] w-[1.4ch] items-center justify-center select-none',
              glyphClass
            )}
          >
            <span className="tty-spin">{effectiveGlyph}</span>
          </span>
        ) : (
          <span
            aria-hidden
            className={cn('inline-block w-[1ch] select-none tabular-nums', glyphClass)}
          >
            {effectiveGlyph}
          </span>
        )}
        <span className={cn('font-bold tracking-[0.06em]', ROLE_TEXT_CLASS[role])}>
          {labelText}
        </span>
        {timestamp !== undefined && (
          <span className="text-muted-foreground-faint tabular-nums">{formatClock(timestamp)}</span>
        )}
        {badges && <span className="flex items-center gap-1.5">{badges}</span>}
        {meta && (
          <span className="ml-auto flex items-center gap-2 text-muted-foreground">{meta}</span>
        )}
      </div>

      {/* BODY: indented 3 columns under the glyph + label gutter */}
      {children !== undefined && children !== null && (
        <div className="pl-[3ch] mt-1 text-foreground/90 [overflow-wrap:anywhere]">
          {children}
          {caret && <span aria-hidden className="tty-caret ml-0.5 inline-block" />}
        </div>
      )}
    </Wrap>
  )
}
