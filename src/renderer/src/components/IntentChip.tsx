import React, { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import type { CognitiveEvent } from '../store/usePairStore'

interface IntentChipProps {
  events: CognitiveEvent[]
  /** Reserved for future role-aware variants (currently unused in render). */
  role?: 'mentor' | 'executor'
  className?: string
}

function getIntent(
  t: (key: string) => string,
  events: CognitiveEvent[]
): { label: string; tone: string } {
  const latest = events[events.length - 1]
  if (!latest) return { label: t('console.processing'), tone: 'text-muted-foreground' }
  if (latest.eventType === 'error')
    return { label: t('console.processingError'), tone: 'state-error' }
  if (latest.eventType === 'reasoning')
    return { label: t('console.reasoning'), tone: 'role-executor' }

  const toolName = latest.toolName?.toLowerCase() ?? ''
  if (toolName.includes('bash'))
    return { label: t('console.runningCommand'), tone: 'state-running' }
  if (toolName.includes('read')) return { label: t('console.readingFile'), tone: 'role-mentor' }
  if (toolName.includes('write')) return { label: t('console.writingCode'), tone: 'state-done' }
  if (toolName.includes('edit')) return { label: t('console.editingFile'), tone: 'state-done' }
  if (toolName.includes('search') || toolName.includes('grep'))
    return { label: t('console.searchingCode'), tone: 'role-mentor' }

  return { label: t('console.processing'), tone: 'text-muted-foreground' }
}

export function IntentChip({ events, className }: IntentChipProps): React.ReactNode {
  const { t } = useTranslation()
  const intent = useMemo(() => getIntent(t, events), [events, t])

  return (
    <span
      className={cn(
        'inline-flex items-baseline gap-1 font-mono text-[10px] uppercase tracking-[0.14em]',
        intent.tone,
        className
      )}
    >
      <span aria-hidden className="tty-blink select-none">
        *
      </span>
      <span>{intent.label.toLowerCase()}</span>
    </span>
  )
}
