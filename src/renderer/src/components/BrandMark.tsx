import React from 'react'
import { cn } from '../lib/utils'

/**
 * The Pair brand mark — two agents facing each other across a `</>`, brewed
 * into a cup of coffee. Drawn once here and reused wherever the brand has to
 * appear (boot splash, empty hub, dialog headers).
 */
export type BrandMarkVariant = 'full' | 'band'

interface BrandMarkProps {
  /** Rendered width in px; the height follows the viewBox. */
  size?: number
  /**
   * 'full' = the whole logo (agents + coffee). 'band' = the agent row only,
   * for places where the mark sits inline next to a title.
   */
  variant?: BrandMarkVariant
  /** Floating agents, blinking eyes, rising steam. */
  animated?: boolean
  className?: string
}

// 'band' crops the logo to the agent row: everything from the antenna tips to
// the chin, with a little breathing room on each side.
const VIEW_BOX: Record<BrandMarkVariant, string> = {
  full: '0 0 320 260',
  band: '32 12 256 96'
}

const ASPECT: Record<BrandMarkVariant, number> = {
  full: 260 / 320,
  band: 96 / 256
}

export function BrandMark({
  size = 200,
  variant = 'full',
  animated = false,
  className
}: BrandMarkProps): React.ReactNode {
  return (
    <svg
      viewBox={VIEW_BOX[variant]}
      width={size}
      height={Math.round(size * ASPECT[variant])}
      fill="none"
      stroke="currentColor"
      strokeWidth="3"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={cn(
        'brand-mark__art state-running',
        // The band crop leaves the coffee cup outside the viewport; without
        // the clip the outer <svg> would paint it over the content below.
        variant === 'band' && 'overflow-hidden',
        animated && 'brand-mark--animated',
        className
      )}
      aria-hidden
    >
      {/* steam wisps — animated upward, behind everything else. Only the full
          mark shows the cup, so the band crop leaves them out entirely. */}
      {variant === 'full' && (
        <g className="brand-mark__steam" strokeWidth="2.6" opacity="0.85">
          <path className="brand-mark__steam-1" d="M148 178 Q144 168 150 158 Q156 148 150 138" />
          <path className="brand-mark__steam-2" d="M160 178 Q156 168 162 158 Q156 148 162 138" />
          <path className="brand-mark__steam-3" d="M172 178 Q168 168 174 158 Q168 148 174 138" />
        </g>
      )}

      {/* left robot (mentor side) */}
      <g className="brand-mark__bot brand-mark__bot--left">
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
        <g className="brand-mark__eyes brand-mark__eyes--left">
          <rect x="67" y="66" width="8" height="8" rx="1.5" fill="currentColor" stroke="none" />
          <rect x="85" y="66" width="8" height="8" rx="1.5" fill="currentColor" stroke="none" />
        </g>
        {/* chin / mouth slot */}
        <line x1="68" y1="92" x2="92" y2="92" strokeWidth="2.2" />
      </g>

      {/* right robot (executor side) — mirrored */}
      <g className="brand-mark__bot brand-mark__bot--right">
        <circle cx="240" cy="24" r="4" fill="currentColor" stroke="none" />
        <line x1="240" y1="28" x2="240" y2="42" />
        <rect x="210" y="42" width="60" height="58" rx="9" ry="9" />
        <circle cx="204" cy="68" r="6" />
        <circle cx="204" cy="68" r="1.6" fill="currentColor" stroke="none" />
        <circle cx="276" cy="68" r="6" />
        <circle cx="276" cy="68" r="1.6" fill="currentColor" stroke="none" />
        <rect x="220" y="58" width="40" height="24" rx="3" ry="3" />
        <g className="brand-mark__eyes brand-mark__eyes--right">
          <rect x="227" y="66" width="8" height="8" rx="1.5" fill="currentColor" stroke="none" />
          <rect x="245" y="66" width="8" height="8" rx="1.5" fill="currentColor" stroke="none" />
        </g>
        <line x1="228" y1="92" x2="252" y2="92" strokeWidth="2.2" />
      </g>

      {/* code symbol between heads — pulsing. Drawn as paths, not <text>:
          the mark has to render the same in every crop, without a font. */}
      <g className="brand-mark__code">
        <polyline points="151,56 142,69 151,82" />
        <line x1="161" y1="56" x2="155" y2="82" />
        <polyline points="169,56 178,69 169,82" />
      </g>

      {/* coffee cup */}
      {variant === 'full' && (
        <g className="brand-mark__cup">
          {/* cup body */}
          <path d="M134 180 L186 180 L181 218 Q160 224 139 218 Z" />
          {/* handle */}
          <path d="M186 187 Q200 191 200 200 Q200 209 186 213" strokeWidth="2.6" />
          {/* saucer */}
          <ellipse cx="160" cy="228" rx="38" ry="4" />
        </g>
      )}
    </svg>
  )
}
