import { cn } from '../../lib/utils'
import { sectionLabel } from '../../lib/ui'

interface StatCardProps {
  value: number | string
  label: string
  color: 'primary' | 'green' | 'amber' | 'gray'
  className?: string
  'data-testid'?: string
}

/** Shell stat tile: sans label, tabular-nums value, card surface. */
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
      className={cn('shell-card flex items-center justify-between px-3.5 py-3', className)}
      data-testid={testId}
    >
      <div className="flex min-w-0 items-center gap-2">
        <span aria-hidden className={cn('shrink-0 select-none text-[10px]', c.text)}>
          {c.glyph}
        </span>
        <span className={cn(sectionLabel, 'truncate')}>{label}</span>
      </div>
      <div className={cn('shrink-0 text-xl tabular-nums leading-none font-semibold', c.text)}>
        {value}
      </div>
    </div>
  )
}
