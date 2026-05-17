import { cn } from '../../lib/utils'

interface StatCardProps {
  value: number | string
  label: string
  color: 'primary' | 'green' | 'amber' | 'gray'
  className?: string
  'data-testid'?: string
}

/**
 * Flat terminal stat tile. Visual lineage from "stat card" is gone but the
 * `glass-card` class (now a CSS shim → flat surface) and grid placement are
 * preserved so existing tests + parent layouts still work.
 */
const colorMap: Record<StatCardProps['color'], { text: string; glyph: string }> = {
  primary: { text: 'text-foreground', glyph: '●' },
  green: { text: 'state-done', glyph: '●' },
  amber: { text: 'state-running', glyph: '*' },
  gray: { text: 'text-muted-foreground', glyph: '○' }
}

export function StatCard({
  value,
  label,
  color,
  className,
  'data-testid': testId
}: StatCardProps): React.ReactNode {
  const c = colorMap[color]
  return (
    <div
      className={cn(
        'glass-card flex items-baseline justify-between px-3 py-2.5 font-mono',
        className
      )}
      data-testid={testId}
    >
      <div className="flex items-baseline gap-2 min-w-0">
        <span aria-hidden className={cn('select-none', c.text)}>
          {c.glyph}
        </span>
        <span className="text-[10px] uppercase tracking-[0.18em] text-muted-foreground truncate">
          {label}
        </span>
      </div>
      <div className={cn('text-lg tabular-nums leading-none font-bold', c.text)}>{value}</div>
    </div>
  )
}
