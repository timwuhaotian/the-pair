import React from 'react'
import { cn } from '../../lib/utils'

interface GlassButtonProps {
  children: React.ReactNode
  onClick?: React.MouseEventHandler<HTMLButtonElement>
  variant?: 'primary' | 'secondary' | 'ghost' | 'destructive'
  size?: 'sm' | 'md' | 'lg'
  className?: string
  disabled?: boolean
  type?: 'button' | 'submit'
  icon?: React.ReactNode
  'aria-label'?: string
  title?: string
  'data-testid'?: string
}

/**
 * Terminal-flat button. Square corners, hairline border, no shadow.
 * Visual lineage of "GlassButton" is gone but the name + API survives
 * so callers don't need updating.
 */
const variantMap: Record<NonNullable<GlassButtonProps['variant']>, string> = {
  primary:
    'bg-primary text-primary-foreground border-primary font-semibold hover:bg-primary/85 hover:border-primary/85',
  secondary:
    'bg-transparent text-foreground border-border hover:bg-foreground/[0.06] hover:border-foreground/40',
  ghost:
    'bg-transparent text-foreground/80 border-transparent hover:bg-foreground/[0.05] hover:text-foreground',
  destructive:
    'bg-transparent text-[var(--state-error)] border-[color:color-mix(in_srgb,var(--state-error)_45%,transparent)] hover:bg-[color:color-mix(in_srgb,var(--state-error)_12%,transparent)]'
}

const sizeMap: Record<NonNullable<GlassButtonProps['size']>, string> = {
  sm: 'px-2 py-[3px] text-[11px] gap-1.5 rounded-sm min-h-[26px]',
  md: 'px-3 py-1.5 text-[12px] gap-2 rounded-sm min-h-[32px]',
  lg: 'px-4 py-2 text-[13px] gap-2 rounded-sm min-h-[38px]'
}

export function GlassButton({
  children,
  onClick,
  variant = 'primary',
  size = 'md',
  className,
  disabled = false,
  type = 'button',
  icon,
  'aria-label': ariaLabel,
  title,
  'data-testid': dataTestId
}: GlassButtonProps): React.ReactNode {
  return (
    <button
      type={type}
      onClick={onClick}
      disabled={disabled}
      aria-label={ariaLabel}
      title={title}
      data-testid={dataTestId}
      className={cn(
        'inline-flex items-center justify-center font-mono uppercase tracking-[0.08em] border transition-colors duration-150 cursor-pointer select-none',
        variantMap[variant],
        sizeMap[size],
        disabled && 'opacity-40 cursor-not-allowed pointer-events-none',
        className
      )}
    >
      {icon && <span className="shrink-0">{icon}</span>}
      {children}
    </button>
  )
}
