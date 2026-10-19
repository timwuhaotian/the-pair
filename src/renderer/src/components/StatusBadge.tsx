import React from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import type { Tone } from '../lib/ui'
import { Chip } from './ui/Chip'

interface StatusBadgeProps {
  status: string
  stalled?: boolean
}

const STATUS_TONE: Record<string, Tone> = {
  Idle: 'neutral',
  Mentoring: 'mentor',
  Executing: 'executor',
  Reviewing: 'running',
  Paused: 'neutral',
  'Awaiting Human Review': 'running',
  Error: 'error',
  Finished: 'done'
}

const STATUS_GLYPH: Record<string, string> = {
  Idle: '○',
  Mentoring: '*',
  Executing: '*',
  Reviewing: '*',
  Paused: '◌',
  'Awaiting Human Review': '!',
  Error: '✗',
  Finished: '✓'
}

export function StatusBadge({ status, stalled }: StatusBadgeProps): React.ReactNode {
  const { t } = useTranslation()
  const tone: Tone = stalled ? 'error' : (STATUS_TONE[status] ?? 'neutral')
  const glyph = stalled ? '!' : (STATUS_GLYPH[status] ?? '·')
  const isActive =
    status === 'Mentoring' || status === 'Executing' || status === 'Reviewing' || stalled

  // Localized label — fall back to raw status if no translation key exists.
  const label = t(`status.${status}`, { defaultValue: status })

  return (
    <Chip shape="pill" tone={tone} title={label}>
      <span aria-hidden className={cn('select-none w-[1ch] tabular-nums', isActive && 'tty-blink')}>
        {glyph}
      </span>
      <span>{label}</span>
    </Chip>
  )
}
