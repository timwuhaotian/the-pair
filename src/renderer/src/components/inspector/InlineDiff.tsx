import React from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '../../lib/utils'

function lineTone(line: string): string {
  if (line.startsWith('+') && !line.startsWith('+++')) return 'state-done bg-state-done'
  if (line.startsWith('-') && !line.startsWith('---')) return 'state-error bg-state-error'
  if (line.startsWith('@@')) return 'role-mentor bg-role-mentor'
  if (
    line.startsWith('diff') ||
    line.startsWith('index') ||
    line.startsWith('---') ||
    line.startsWith('+++')
  )
    return 'text-muted-foreground-faint'
  return 'text-foreground/80'
}

interface InlineDiffProps {
  diff: string | null
  loading: boolean
  error: string | null
}

/** Shared diff body used inline in the inspector (and formerly the diff modal). */
export function InlineDiff({ diff, loading, error }: InlineDiffProps): React.ReactNode {
  const { t } = useTranslation()
  if (loading) {
    return (
      <div className="flex items-baseline justify-start gap-2 px-2 py-2 font-mono text-[10px] text-muted-foreground">
        <span className="state-running tty-blink">●</span>
        <span>{t('pair.loadingDiff')}</span>
      </div>
    )
  }
  if (error) {
    return <div className="px-2 py-1.5 font-mono text-[10px] state-error">✗ {error}</div>
  }
  if (!diff) return null
  return (
    <div className="overflow-x-auto py-1">
      {diff.split('\n').map((line, i) => (
        <div
          key={i}
          className={cn(
            'whitespace-pre font-mono text-[11px] leading-snug px-2 py-px',
            lineTone(line)
          )}
        >
          {line || ' '}
        </div>
      ))}
    </div>
  )
}
