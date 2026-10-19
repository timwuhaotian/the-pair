import React from 'react'
import { cn } from '../../lib/utils'
import { chipClass, toneClasses, type ChipShape, type Tone } from '../../lib/ui'

interface ChipProps {
  children: React.ReactNode
  tone?: Tone
  shape?: ChipShape
  /** Drop the tint and keep only the colored hairline (low-emphasis tags). */
  outline?: boolean
  title?: string
  className?: string
  'data-testid'?: string
}

/** Status badge (`pill`) or compact stream/inspector tag (`tag`). */
export function Chip({
  children,
  tone = 'neutral',
  shape = 'tag',
  outline = false,
  title,
  className,
  'data-testid': dataTestId
}: ChipProps): React.ReactNode {
  const c = toneClasses(tone)
  return (
    <span
      title={title}
      data-testid={dataTestId}
      className={cn(chipClass(shape), c.fg, c.border, outline ? 'bg-transparent' : c.bg, className)}
    >
      {children}
    </span>
  )
}
