import React, { useId } from 'react'
import { cn } from '../../lib/utils'
import { BRAND_ICONS, type BrandIconData, type BrandIconKey } from '../../lib/brandIcons'

interface BrandIconProps {
  /** Mark to draw; when undefined a monogram of `label` stands in. */
  icon?: BrandIconKey
  /** Brand name, used for the monogram and as the accessible name. */
  label: string
  /** Tailwind size classes; defaults to a 14px square. */
  className?: string
  /** Decorative marks sit beside visible text and stay hidden from AT. */
  decorative?: boolean
}

export function BrandIcon({
  icon,
  label,
  className,
  decorative = true
}: BrandIconProps): React.ReactNode {
  const uid = useId().replace(/[^a-zA-Z0-9_-]/g, '')
  const a11y = decorative
    ? { 'aria-hidden': true as const }
    : { role: 'img' as const, 'aria-label': label }

  if (!icon) {
    return (
      <span
        {...a11y}
        className={cn(
          'inline-flex size-3.5 shrink-0 select-none items-center justify-center rounded-[3px] border border-current text-[8px] font-semibold uppercase leading-none text-muted-foreground',
          className
        )}
      >
        {label.trim().charAt(0) || '?'}
      </span>
    )
  }

  const data: BrandIconData = BRAND_ICONS[icon]
  return (
    <svg
      {...a11y}
      viewBox="0 0 24 24"
      fill={data.fill}
      fillRule={data.fillRule}
      className={cn('size-3.5 shrink-0', className)}
      // Static, vetted vendor markup from brandIcons.ts — never user input.
      dangerouslySetInnerHTML={{ __html: data.markup.replaceAll('__UID__', uid) }}
    />
  )
}
