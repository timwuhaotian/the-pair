import React from 'react'
import { X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { motion, AnimatePresence } from 'framer-motion'

interface ReasoningEffortPickerProps {
  levels: string[]
  value?: string
  onChange: (value: string | undefined) => void
  role: 'mentor' | 'executor'
}

const EFFORT_LABELS: Record<string, string> = {
  none: 'Off',
  low: 'Fast',
  medium: 'Balanced',
  high: 'Deep'
}

const ROLE_TONE = {
  mentor: {
    activeBg: 'bg-blue-500/15 dark:bg-blue-500/20',
    activeText: 'text-blue-600 dark:text-blue-400',
    activeBorder: 'border-blue-500/40',
    inactiveBg: 'bg-transparent',
    inactiveText: 'text-muted-foreground',
    inactiveBorder: 'border-transparent',
    labelText: 'text-blue-600 dark:text-blue-400'
  },
  executor: {
    activeBg: 'bg-purple-500/15 dark:bg-purple-500/20',
    activeText: 'text-purple-600 dark:text-purple-400',
    activeBorder: 'border-purple-500/40',
    inactiveBg: 'bg-transparent',
    inactiveText: 'text-muted-foreground',
    inactiveBorder: 'border-transparent',
    labelText: 'text-purple-600 dark:text-purple-400'
  }
} as const

export function ReasoningEffortPicker({
  levels,
  value,
  onChange,
  role
}: ReasoningEffortPickerProps): React.ReactNode {
  const { t } = useTranslation()
  const tone = ROLE_TONE[role]

  const effortLabel = (level: string): string => {
    const key = `pickers.reasoning${level.charAt(0).toUpperCase() + level.slice(1)}` as const
    const translated = t(key)
    return translated !== key ? translated : (EFFORT_LABELS[level] ?? level)
  }

  const handleSelect = (level: string): void => {
    if (level === value) {
      onChange(undefined)
    } else {
      onChange(level)
    }
  }

  const handleClear = (): void => {
    onChange(undefined)
  }

  if (levels.length === 0) return null

  return (
    <AnimatePresence>
      <motion.div
        initial={{ opacity: 0, y: -4 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, y: -4 }}
        transition={{ duration: 0.15 }}
        className="space-y-1.5"
      >
        <div className="flex items-center gap-1.5">
          <span
            className={cn('text-[10px] font-semibold uppercase tracking-wider', tone.labelText)}
          >
            {t('pickers.reasoning')}
          </span>
          <span className="text-[10px] text-muted-foreground">—</span>
          <AnimatePresence>
            {value && (
              <motion.span
                initial={{ opacity: 0, scale: 0.8 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.8 }}
                className={cn('text-[10px] font-semibold', tone.activeText)}
              >
                {value && effortLabel(value)}
              </motion.span>
            )}
          </AnimatePresence>
        </div>
        <div className="flex items-center gap-1">
          <div
            className={cn('flex flex-1 rounded-lg border', tone.inactiveBorder, 'overflow-hidden')}
          >
            {levels.map((level) => {
              const isActive = level === value
              return (
                <button
                  key={level}
                  type="button"
                  onClick={() => handleSelect(level)}
                  className={cn(
                    'flex-1 px-2 py-1 text-[10px] font-medium transition-all cursor-pointer',
                    isActive
                      ? cn(tone.activeBg, tone.activeText)
                      : cn(tone.inactiveBg, tone.inactiveText, 'hover:bg-muted/40')
                  )}
                >
                  {effortLabel(level)}
                </button>
              )
            })}
          </div>
          {value && (
            <motion.button
              initial={{ opacity: 0, scale: 0.8 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.8 }}
              type="button"
              onClick={handleClear}
              className={cn(
                'flex h-6 w-6 items-center justify-center rounded-md border cursor-pointer',
                tone.inactiveBorder,
                'text-muted-foreground hover:bg-muted/40 transition-all'
              )}
              title={t('pickers.resetReasoning')}
            >
              <X size={10} />
            </motion.button>
          )}
        </div>
      </motion.div>
    </AnimatePresence>
  )
}
