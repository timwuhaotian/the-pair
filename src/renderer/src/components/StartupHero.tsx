import React from 'react'
import { cn } from '../lib/utils'
import { BrandMark } from './BrandMark'

interface StartupHeroProps {
  size?: 'sm' | 'md' | 'lg'
  tagline?: string
  wordmark?: string
  caption?: string
  animated?: boolean
  className?: string
  showText?: boolean
}

const SIZE_TO_WIDTH: Record<NonNullable<StartupHeroProps['size']>, number> = {
  sm: 200,
  md: 280,
  lg: 360
}

export function StartupHero({
  size = 'md',
  tagline = 'Mentor plans. Executor ships.',
  wordmark = 'THE PAIR',
  caption,
  animated = true,
  className,
  showText = true
}: StartupHeroProps): React.ReactNode {
  const width = SIZE_TO_WIDTH[size]
  const animClass = animated ? 'startup-hero--animated' : ''

  return (
    <div
      className={cn(
        'startup-hero flex flex-col items-center font-mono select-none',
        animClass,
        className
      )}
      role="img"
      aria-label={`${wordmark} — ${tagline}`}
    >
      <BrandMark size={width} animated={animated} />

      {showText && (
        <div className="startup-hero__text mt-4 flex flex-col items-center gap-1.5 text-center">
          <div className="flex items-baseline gap-2 text-[10px] uppercase tracking-[0.32em] text-muted-foreground-faint">
            <span aria-hidden>──</span>
            <span>{wordmark.toLowerCase()}</span>
            <span aria-hidden>──</span>
          </div>
          <p className="text-[13px] font-bold tracking-[0.04em] text-foreground/90">{tagline}</p>
          {caption && (
            <p className="text-[10px] uppercase tracking-[0.22em] text-muted-foreground">
              {caption}
            </p>
          )}
        </div>
      )}
    </div>
  )
}
