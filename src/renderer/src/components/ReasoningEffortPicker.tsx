import React from 'react'
import { X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'

interface ReasoningEffortPickerProps {
  levels: string[]
  value?: string
  onChange: (value: string | undefined) => void
  role: 'mentor' | 'executor'
}

const EFFORT_LABELS: Record<string, string> = {
  none: 'off',
  low: 'fast',
  medium: 'balanced',
  high: 'deep'
}

export function ReasoningEffortPicker({
  levels,
  value,
  onChange,
  role
}: ReasoningEffortPickerProps): React.ReactNode {
  const { t } = useTranslation()

  const roleFg = role === 'mentor' ? 'role-mentor' : 'role-executor'
  const roleBg = role === 'mentor' ? 'bg-role-mentor' : 'bg-role-executor'
  const roleBorder = role === 'mentor' ? 'border-role-mentor' : 'border-role-executor'

  const effortLabel = (level: string): string => {
    const key = `pickers.reasoning${level.charAt(0).toUpperCase() + level.slice(1)}` as const
    const translated = t(key)
    return translated !== key
      ? translated.toLowerCase()
      : (EFFORT_LABELS[level] ?? level.toLowerCase())
  }

  const handleSelect = (level: string): void => {
    if (level === value) onChange(undefined)
    else onChange(level)
  }

  const handleClear = (): void => onChange(undefined)

  if (levels.length === 0) return null

  return (
    <div className="space-y-1 font-mono">
      <div className="flex items-baseline gap-2">
        <span className={cn('text-[10px] uppercase tracking-[0.16em]', roleFg)}>
          {t('pickers.reasoning')}
        </span>
        {value && (
          <span className={cn('text-[10px] uppercase tracking-[0.14em]', roleFg)}>
            · {effortLabel(value)}
          </span>
        )}
      </div>
      <div className="flex items-center gap-1">
        <div className="flex flex-1 border border-border rounded-sm overflow-hidden">
          {levels.map((level) => {
            const isActive = level === value
            return (
              <button
                key={level}
                type="button"
                onClick={() => handleSelect(level)}
                className={cn(
                  'flex-1 px-2 py-0.5 text-[11px] transition-colors cursor-pointer border-r border-border last:border-r-0',
                  isActive
                    ? cn(roleBg, roleFg, roleBorder)
                    : 'text-muted-foreground hover:bg-foreground/[0.05] hover:text-foreground'
                )}
              >
                {effortLabel(level)}
              </button>
            )
          })}
        </div>
        {value && (
          <button
            type="button"
            onClick={handleClear}
            className="flex h-5 w-5 items-center justify-center border border-border rounded-sm text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground transition-colors cursor-pointer"
            title={t('pickers.resetReasoning')}
          >
            <X size={9} />
          </button>
        )}
      </div>
    </div>
  )
}
