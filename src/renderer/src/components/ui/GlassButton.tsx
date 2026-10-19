import React from 'react'
import { cn } from '../../lib/utils'

interface GlassButtonProps {
  children: React.ReactNode
  onClick?: React.MouseEventHandler<HTMLButtonElement>
  variant?: 'primary' | 'secondary' | 'ghost' | 'destructive'
  size?: 'xs' | 'sm' | 'md' | 'lg'
  className?: string
  disabled?: boolean
  type?: 'button' | 'submit'
  icon?: React.ReactNode
  'aria-label'?: string
  title?: string
  'data-testid'?: string
}

/**
 * Shell button. Sans type, medium radius, hairline border. Case follows the
 * label copy (sentence case) rather than forced uppercase — the old terminal
 * uppercase style survives only inside the conversation core.
 */
const variantMap: Record<
  NonNullable<GlassButtonProps['variant']>,
  { base: string; hover: string }
> = {
  primary: {
    // --ring equals --primary, so the global focus ring would vanish into the
    // fill. Lives in `base`, not `hover`, which disabled buttons drop.
    base: 'bg-primary text-primary-foreground border-primary font-medium focus-visible:outline-[color:var(--foreground)]',
    hover: 'hover:bg-primary/85 hover:border-primary/85'
  },
  secondary: {
    base: 'bg-transparent text-foreground border-border',
    hover: 'hover:bg-foreground/[0.05] hover:border-foreground/40'
  },
  ghost: {
    base: 'bg-transparent text-foreground/80 border-transparent',
    hover: 'hover:bg-foreground/[0.05] hover:text-foreground'
  },
  destructive: {
    base: 'bg-transparent state-error border-state-error/45',
    hover: 'hover:bg-state-error/12'
  }
}

const sizeMap: Record<NonNullable<GlassButtonProps['size']>, string> = {
  // xs: compact control inside the conversation core / inspector rows.
  xs: 'px-1.5 py-0.5 text-[11px] gap-1 rounded-sm min-h-[22px]',
  sm: 'px-2.5 py-1 text-[12px] gap-1.5 rounded-md min-h-[28px]',
  md: 'px-3.5 py-1.5 text-[13px] gap-2 rounded-md min-h-[34px]',
  lg: 'px-4 py-2 text-[14px] gap-2 rounded-md min-h-[40px]'
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
        'inline-flex cursor-pointer items-center justify-center border transition-colors duration-150 select-none',
        variantMap[variant].base,
        sizeMap[size],
        // No pointer-events-none: a disabled button's `title` is often the only
        // place the "why is this disabled" reason is shown. Hover styles are
        // simply withheld instead.
        disabled ? 'cursor-not-allowed opacity-40' : variantMap[variant].hover,
        className
      )}
    >
      {icon && <span className="shrink-0">{icon}</span>}
      {children}
    </button>
  )
}
