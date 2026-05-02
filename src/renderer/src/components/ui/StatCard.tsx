import { cn } from '../../lib/utils'

interface StatCardProps {
  value: number | string
  label: string
  color: 'primary' | 'green' | 'amber' | 'gray'
  className?: string
  'data-testid'?: string
}

const colorMap: Record<StatCardProps['color'], string> = {
  primary: 'from-primary/10 to-primary/5 border-primary/20 text-primary',
  green: 'from-green-500/10 to-green-500/5 border-green-500/20 text-green-500',
  amber: 'from-amber-500/10 to-amber-500/5 border-amber-500/20 text-amber-500',
  gray: 'from-gray-500/10 to-gray-500/5 border-gray-500/20 text-gray-500'
}

export function StatCard({
  value,
  label,
  color,
  className,
  'data-testid': testId
}: StatCardProps): React.ReactNode {
  return (
    <div
      className={cn(
        'glass-card flex flex-col items-center gap-1 rounded-xl border bg-gradient-to-br p-4',
        colorMap[color],
        className
      )}
      data-testid={testId}
    >
      <div className="text-2xl font-semibold tracking-tight">{value}</div>
      <div className="text-xs font-medium uppercase tracking-wider opacity-70">{label}</div>
    </div>
  )
}
