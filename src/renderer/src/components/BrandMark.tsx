import React from 'react'
import { cn } from '../lib/utils'

/**
 * The Pair brand mark — the family mark killi, kil and kiled share, made a
 * pair: two of killi's tiles side by side, each holding one check of the app
 * icon's double check, with killi's coral stick behind and gold spark beside.
 * Drawn once here and reused wherever the brand has to appear (boot splash,
 * empty hub, dialog headers).
 *
 * The colors are the family's own and do not follow the theme: the tile reads
 * on both the light and the dark background, as kil's does in the Dock.
 */
export type BrandMarkVariant = 'full' | 'band'

interface BrandMarkProps {
  /** Rendered width in px; the height follows the viewBox. */
  size?: number
  /**
   * 'full' = the whole mark (tiles, stick, spark). 'band' = the two tiles
   * only, for places where the mark sits inline next to a title.
   */
  variant?: BrandMarkVariant
  /**
   * The launch sequence — each tile lands and its check draws, mentor first,
   * then the stick rises and the spark pops — followed by a slow idle bob.
   */
  animated?: boolean
  className?: string
}

// 'band' is the box around the two tiles, with room for their bob.
const VIEW_BOX: Record<BrandMarkVariant, string> = {
  full: '0 0 720 456',
  band: '51 116 624 332'
}

const ASPECT: Record<BrandMarkVariant, number> = {
  full: 456 / 720,
  band: 332 / 624
}

// Where each tile sits: an x offset from killi's own position in its icon.
const TILE_OFFSET_X = { mentor: -51, executor: 259 } as const

interface TileProps {
  role: keyof typeof TILE_OFFSET_X
  className: string
}

/**
 * One tile. The inner group is killi's (../killi/docs/icon.svg) — same
 * colors, -5° tilt and proportions — with a check where killi has its prompt.
 * The outer group carries no transform of its own, so CSS can animate it
 * without replacing the placement.
 */
function Tile({ role, className }: TileProps): React.ReactNode {
  return (
    <g className={cn('brand-mark__tile', className)}>
      <g transform={`translate(${TILE_OFFSET_X[role]} -22) rotate(-5 256 292)`}>
        <rect x="120" y="176" width="276" height="276" rx="72" fill="#6FA8DC" />
        <rect x="120" y="156" width="276" height="276" rx="72" fill="#CFE2F3" />
        <rect
          x="154"
          y="190"
          width="70"
          height="23"
          rx="11.5"
          fill="#FFFFFF"
          transform="rotate(-28 189 201)"
        />
        <circle cx="244" cy="194" r="9" fill="#FFFFFF" />
        <ellipse cx="338" cy="358" rx="21" ry="14" fill="#F4CCCC" />
        {/* pathLength lets the stylesheet draw the stroke from 1 to 0. */}
        <path
          className="brand-mark__check"
          d="M 194 302 L 238 346 L 322 240"
          pathLength={1}
          fill="none"
          stroke="#223853"
          strokeWidth="34"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </g>
    </g>
  )
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
      className={cn('brand-mark__art', animated && 'brand-mark--animated', className)}
      aria-hidden
    >
      {/* killi's stick and spark, behind and beside the tiles. They lie
          outside the band, so that crop leaves them out. */}
      {variant === 'full' && (
        <>
          <g className="brand-mark__stick">
            <rect
              x="561"
              y="16"
              width="46"
              height="230"
              rx="23"
              fill="#EF6868"
              transform="rotate(18 584 131)"
            />
          </g>
          <g className="brand-mark__spark">
            <path
              d="M 37 358 L 44.8 380.2 L 67 388 L 44.8 395.8 L 37 418 L 29.2 395.8 L 7 388 L 29.2 380.2 Z"
              fill="#E09C26"
            />
          </g>
        </>
      )}

      <Tile role="mentor" className="brand-mark__tile--mentor" />
      <Tile role="executor" className="brand-mark__tile--executor" />
    </svg>
  )
}
