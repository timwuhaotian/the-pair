import React from 'react'
import { cn } from '../lib/utils'

interface StatusBadgeProps {
  status: string
  stalled?: boolean
}

export function StatusBadge({ status, stalled }: StatusBadgeProps): React.ReactNode {
  const isStalled = stalled || false

  const config: Record<string, { bg: string; text: string; border: string; glow: string }> = {
    Idle: {
      bg: 'bg-muted dark:bg-white/8',
      text: 'text-muted-foreground dark:text-white/70',
      border: 'border-border dark:border-white/12',
      glow: ''
    },
    Mentoring: {
      bg: 'bg-blue-500/15 dark:bg-blue-500/20',
      text: 'text-blue-700 dark:text-blue-300',
      border: 'border-blue-500/30 dark:border-blue-500/35',
      glow: 'shadow-blue-500/25 dark:shadow-blue-500/30 shadow-[0_0_14px_rgba(59,130,246,0.25)]'
    },
    Executing: {
      bg: 'bg-purple-500/15 dark:bg-purple-500/20',
      text: 'text-purple-700 dark:text-purple-300',
      border: 'border-purple-500/30 dark:border-purple-500/35',
      glow: 'shadow-purple-500/25 dark:shadow-purple-500/30 shadow-[0_0_14px_rgba(168,85,247,0.25)]'
    },
    Reviewing: {
      bg: 'bg-amber-500/15 dark:bg-amber-500/20',
      text: 'text-amber-700 dark:text-amber-300',
      border: 'border-amber-500/30 dark:border-amber-500/35',
      glow: 'shadow-amber-500/25 dark:shadow-amber-500/30 shadow-[0_0_14px_rgba(245,158,11,0.25)]'
    },
    Paused: {
      bg: 'bg-slate-500/15 dark:bg-slate-400/15',
      text: 'text-slate-700 dark:text-slate-200',
      border: 'border-slate-500/25 dark:border-slate-400/25',
      glow: 'shadow-slate-500/15 dark:shadow-slate-400/15 shadow-[0_0_12px_rgba(100,116,139,0.15)]'
    },
    'Awaiting Human Review': {
      bg: 'bg-orange-500/20 dark:bg-orange-500/25',
      text: 'text-orange-700 dark:text-orange-300',
      border: 'border-orange-500/35 dark:border-orange-500/40',
      glow: 'shadow-orange-500/30 dark:shadow-orange-500/35 shadow-[0_0_18px_rgba(249,115,22,0.3)]'
    },
    Error: {
      bg: 'bg-red-500/15 dark:bg-red-500/20',
      text: 'text-red-700 dark:text-red-300',
      border: 'border-red-500/30 dark:border-red-500/35',
      glow: 'shadow-red-500/25 dark:shadow-red-500/30 shadow-[0_0_14px_rgba(239,68,68,0.25)]'
    },
    Finished: {
      bg: 'bg-green-500/15 dark:bg-green-500/20',
      text: 'text-green-700 dark:text-green-300',
      border: 'border-green-500/30 dark:border-green-500/35',
      glow: 'shadow-green-500/25 dark:shadow-green-500/30 shadow-[0_0_14px_rgba(34,197,94,0.25)]'
    }
  }

  const s = config[status] || config['Idle']
  const isActive = status === 'Mentoring' || status === 'Executing'

  const pulseColor = isStalled
    ? 'bg-red-400'
    : status === 'Mentoring'
      ? 'bg-blue-400'
      : status === 'Executing'
        ? 'bg-purple-400'
        : null

  return (
    <span
      className={cn(
        'text-[10px] font-medium px-2.5 py-1.5 rounded-full border inline-flex items-center gap-1.5',
        s.bg,
        isStalled
          ? 'bg-red-500/20 dark:bg-red-500/25 text-red-400 border-red-500/40 dark:border-red-500/45 shadow-red-500/20 dark:shadow-red-500/25 shadow-[0_0_14px_rgba(239,68,68,0.2)]'
          : s.text,
        s.border,
        isStalled
          ? 'border-red-500/40 dark:border-red-500/45 shadow-red-500/20 dark:shadow-red-500/25 shadow-[0_0_14px_rgba(239,68,68,0.2)]'
          : s.glow
      )}
    >
      {isActive && pulseColor && (
        <span className="relative flex h-1.5 w-1.5">
          <span
            className={cn(
              'animate-ping absolute inline-flex h-full w-full rounded-full opacity-75',
              pulseColor
            )}
            style={{ animationDuration: '2s' }}
          />
          <span
            className={cn(
              'relative inline-flex rounded-full h-1.5 w-1.5',
              pulseColor.replace('400', '500')
            )}
          />
        </span>
      )}
      {status}
    </span>
  )
}
