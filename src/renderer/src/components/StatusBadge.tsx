import React from 'react'
import { cn } from '../lib/utils'

interface StatusBadgeProps {
  status: string
  stalled?: boolean
}

type Tone = 'mentor' | 'executor' | 'running' | 'done' | 'error' | 'muted'

const STATUS_TONE: Record<string, Tone> = {
  Idle: 'muted',
  Mentoring: 'mentor',
  Executing: 'executor',
  Reviewing: 'running',
  Paused: 'muted',
  'Awaiting Human Review': 'running',
  Error: 'error',
  Finished: 'done'
}

const STATUS_GLYPH: Record<string, string> = {
  Idle: '○',
  Mentoring: '*',
  Executing: '*',
  Reviewing: '*',
  Paused: '◌',
  'Awaiting Human Review': '!',
  Error: '✗',
  Finished: '✓'
}

function toneClasses(tone: Tone): { fg: string; bg: string; border: string } {
  switch (tone) {
    case 'mentor':
      return { fg: 'role-mentor', bg: 'bg-role-mentor', border: 'border-role-mentor' }
    case 'executor':
      return { fg: 'role-executor', bg: 'bg-role-executor', border: 'border-role-executor' }
    case 'running':
      return { fg: 'state-running', bg: 'bg-state-running', border: 'border-state-running' }
    case 'done':
      return { fg: 'state-done', bg: 'bg-state-done', border: 'border-state-done' }
    case 'error':
      return { fg: 'state-error', bg: 'bg-state-error', border: 'border-state-error' }
    default:
      return {
        fg: 'text-muted-foreground',
        bg: 'bg-muted',
        border: 'border-border'
      }
  }
}

export function StatusBadge({ status, stalled }: StatusBadgeProps): React.ReactNode {
  const tone: Tone = stalled ? 'error' : (STATUS_TONE[status] ?? 'muted')
  const glyph = stalled ? '!' : (STATUS_GLYPH[status] ?? '·')
  const c = toneClasses(tone)
  const isActive =
    status === 'Mentoring' || status === 'Executing' || status === 'Reviewing' || stalled

  return (
    <span
      className={cn(
        'inline-flex items-baseline gap-1 border rounded-sm px-1.5 py-px font-mono text-[10px] uppercase tracking-[0.14em]',
        c.fg,
        c.bg,
        c.border
      )}
      title={status}
    >
      <span aria-hidden className={cn('select-none w-[1ch] tabular-nums', isActive && 'tty-blink')}>
        {glyph}
      </span>
      <span>{status}</span>
    </span>
  )
}
