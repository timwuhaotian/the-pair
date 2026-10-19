import React from 'react'
import { cn } from '../../lib/utils'
import { hoverTint } from '../../lib/ui'

interface IconButtonProps {
  /** Required: icon-only controls have no visible text to name them. */
  'aria-label': string
  children: React.ReactNode
  onClick?: React.MouseEventHandler<HTMLButtonElement>
  size?: 'sm' | 'md'
  /** Hairline border, for toolbar buttons that sit beside bordered controls. */
  bordered?: boolean
  /** Danger tint on hover (delete / clear). */
  destructive?: boolean
  active?: boolean
  disabled?: boolean
  title?: string
  className?: string
  'aria-pressed'?: boolean
  'aria-expanded'?: boolean
  'aria-haspopup'?: 'menu' | 'listbox' | 'dialog'
  'data-testid'?: string
}

const sizeMap: Record<NonNullable<IconButtonProps['size']>, string> = {
  sm: 'h-6 w-6',
  md: 'h-7 w-7'
}

/** Square icon-only button. Tooltip defaults to the accessible name. */
export const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  {
    'aria-label': ariaLabel,
    children,
    onClick,
    size = 'sm',
    bordered = false,
    destructive = false,
    active = false,
    disabled = false,
    title,
    className,
    'aria-pressed': ariaPressed,
    'aria-expanded': ariaExpanded,
    'aria-haspopup': ariaHaspopup,
    'data-testid': dataTestId
  },
  ref
) {
  return (
    <button
      ref={ref}
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={ariaLabel}
      aria-pressed={ariaPressed}
      aria-expanded={ariaExpanded}
      aria-haspopup={ariaHaspopup}
      title={title ?? ariaLabel}
      data-testid={dataTestId}
      className={cn(
        'inline-flex shrink-0 cursor-pointer items-center justify-center rounded-md text-muted-foreground transition-colors',
        sizeMap[size],
        bordered ? 'border border-border' : 'border border-transparent',
        active && 'bg-foreground/[0.08] text-foreground',
        disabled
          ? 'cursor-not-allowed opacity-40'
          : destructive
            ? 'hover:bg-state-error/10 hover:text-state-error'
            : cn(hoverTint, 'hover:text-foreground', bordered && 'hover:border-foreground/40'),
        className
      )}
    >
      {children}
    </button>
  )
})
