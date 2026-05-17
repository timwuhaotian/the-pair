import React from 'react'
import { cn } from '../../lib/utils'

interface GlassCardProps {
  children: React.ReactNode
  className?: string
  hoverable?: boolean
  onClick?: () => void
  /** Retained for API parity; glow now resolves to a tone-tinted hairline border. */
  glow?: 'blue' | 'purple' | 'green' | 'amber' | 'none'
}

const glowBorder: Record<NonNullable<GlassCardProps['glow']>, string> = {
  blue: 'border-role-mentor',
  purple: 'border-role-executor',
  green: 'border-state-done',
  amber: 'border-state-running',
  none: ''
}

export function GlassCard({
  children,
  className,
  hoverable = false,
  onClick,
  glow = 'none'
}: GlassCardProps): React.ReactNode {
  return (
    <div
      onClick={onClick}
      className={cn(
        'glass-card p-3 font-mono text-[12px]',
        hoverable && 'glass-card-hover cursor-pointer',
        glow !== 'none' && glowBorder[glow],
        onClick && 'cursor-pointer',
        className
      )}
    >
      {children}
    </div>
  )
}
