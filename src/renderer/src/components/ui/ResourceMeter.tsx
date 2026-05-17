import React from 'react'
import { cn } from '../../lib/utils'

interface ResourceMeterProps {
  cpu: number
  mem: number
  className?: string
  compact?: boolean
  hideLabels?: boolean
  cells?: number
}

const DEFAULT_CELLS = 14

export function ResourceMeter({
  cpu,
  mem,
  className,
  compact,
  hideLabels,
  cells = DEFAULT_CELLS
}: ResourceMeterProps): React.ReactNode {
  return (
    <div className={cn('font-mono space-y-1', className)}>
      <MeterRow
        label="cpu"
        value={cpu}
        max={100}
        unit="%"
        cells={cells}
        compact={compact}
        hideLabel={hideLabels}
      />
      <MeterRow
        label="mem"
        value={mem}
        max={8192}
        unit="MB"
        cells={cells}
        compact={compact}
        hideLabel={hideLabels}
      />
    </div>
  )
}

interface MeterRowProps {
  label: string
  value: number
  max: number
  unit: string
  cells: number
  compact?: boolean
  hideLabel?: boolean
}

function MeterRow({
  label,
  value,
  max,
  unit,
  cells,
  compact,
  hideLabel
}: MeterRowProps): React.ReactNode {
  const ratio = Math.max(0, Math.min(value / max, 1))
  const filled = Math.round(ratio * cells)
  const empty = cells - filled
  const isHigh = ratio >= 0.85

  return (
    <div className={cn('flex items-baseline gap-2', compact ? 'text-[10px]' : 'text-[11px]')}>
      {!hideLabel && (
        <span className="w-[3ch] uppercase tracking-[0.16em] text-muted-foreground">{label}</span>
      )}
      <span
        className={cn('tabular-nums leading-none', isHigh ? 'state-running' : 'text-foreground/80')}
      >
        [{'█'.repeat(filled)}
        <span className="text-muted-foreground/30">{'░'.repeat(empty)}</span>]
      </span>
      <span className={cn('ml-auto tabular-nums', isHigh ? 'state-running' : 'text-foreground/85')}>
        {value.toFixed(1)}
        <span className="ml-0.5 text-muted-foreground-faint">{unit}</span>
      </span>
    </div>
  )
}
