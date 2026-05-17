import React from 'react'
import { cn } from '../lib/utils'

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
      <svg
        viewBox="0 0 320 260"
        width={width}
        height={Math.round((width * 260) / 320)}
        fill="none"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
        strokeLinejoin="round"
        className="startup-hero__art"
        style={{ color: 'var(--state-running)' }}
        aria-hidden
      >
        {/* steam wisps — animated upward, behind everything else */}
        <g className="startup-hero__steam" strokeWidth="2.6" opacity="0.85">
          <path className="startup-hero__steam-1" d="M148 178 Q144 168 150 158 Q156 148 150 138" />
          <path className="startup-hero__steam-2" d="M160 178 Q156 168 162 158 Q156 148 162 138" />
          <path className="startup-hero__steam-3" d="M172 178 Q168 168 174 158 Q168 148 174 138" />
        </g>

        {/* left robot (mentor side) */}
        <g className="startup-hero__bot startup-hero__bot--left">
          {/* antenna */}
          <circle cx="80" cy="24" r="4" fill="currentColor" stroke="none" />
          <line x1="80" y1="28" x2="80" y2="42" />
          {/* head */}
          <rect x="50" y="42" width="60" height="58" rx="9" ry="9" />
          {/* ears */}
          <circle cx="44" cy="68" r="6" />
          <circle cx="44" cy="68" r="1.6" fill="currentColor" stroke="none" />
          <circle cx="116" cy="68" r="6" />
          <circle cx="116" cy="68" r="1.6" fill="currentColor" stroke="none" />
          {/* eye screen */}
          <rect x="60" y="58" width="40" height="24" rx="3" ry="3" />
          {/* eyes — animated blink */}
          <g className="startup-hero__eyes startup-hero__eyes--left">
            <rect x="67" y="66" width="8" height="8" rx="1.5" fill="currentColor" stroke="none" />
            <rect x="85" y="66" width="8" height="8" rx="1.5" fill="currentColor" stroke="none" />
          </g>
          {/* chin / mouth slot */}
          <line x1="68" y1="92" x2="92" y2="92" strokeWidth="2.2" />
        </g>

        {/* right robot (executor side) — mirrored */}
        <g className="startup-hero__bot startup-hero__bot--right">
          <circle cx="240" cy="24" r="4" fill="currentColor" stroke="none" />
          <line x1="240" y1="28" x2="240" y2="42" />
          <rect x="210" y="42" width="60" height="58" rx="9" ry="9" />
          <circle cx="204" cy="68" r="6" />
          <circle cx="204" cy="68" r="1.6" fill="currentColor" stroke="none" />
          <circle cx="276" cy="68" r="6" />
          <circle cx="276" cy="68" r="1.6" fill="currentColor" stroke="none" />
          <rect x="220" y="58" width="40" height="24" rx="3" ry="3" />
          <g className="startup-hero__eyes startup-hero__eyes--right">
            <rect x="227" y="66" width="8" height="8" rx="1.5" fill="currentColor" stroke="none" />
            <rect x="245" y="66" width="8" height="8" rx="1.5" fill="currentColor" stroke="none" />
          </g>
          <line x1="228" y1="92" x2="252" y2="92" strokeWidth="2.2" />
        </g>

        {/* code symbol between heads — pulsing */}
        <g className="startup-hero__code">
          <text
            x="160"
            y="80"
            textAnchor="middle"
            fontFamily="var(--font-mono), ui-monospace, monospace"
            fontWeight="700"
            fontSize="22"
            fill="currentColor"
            stroke="none"
            letterSpacing="-0.5"
          >
            {'</>'}
          </text>
        </g>

        {/* coffee cup */}
        <g className="startup-hero__cup">
          {/* cup body */}
          <path d="M134 180 L186 180 L181 218 Q160 224 139 218 Z" />
          {/* handle */}
          <path d="M186 187 Q200 191 200 200 Q200 209 186 213" strokeWidth="2.6" />
          {/* saucer */}
          <ellipse cx="160" cy="228" rx="38" ry="4" />
        </g>
      </svg>

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
