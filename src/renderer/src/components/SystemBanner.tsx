import React from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { MarkdownContent } from './MarkdownContent'
import { TerminalBlock } from './terminal/TerminalBlock'
import { TerminalDivider } from './terminal/TerminalDivider'

type BannerVariant = 'mission' | 'iteration' | 'handoff' | 'human-feedback'

interface SystemBannerProps {
  variant: BannerVariant
  content?: string
  iteration?: number
  maxIterations?: number
  timestamp?: number
  className?: string
}

function formatClock(ts: number): string {
  const d = new Date(ts)
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map((n) => n.toString().padStart(2, '0'))
    .join(':')
}

export function SystemBanner({
  variant,
  content,
  iteration,
  maxIterations,
  timestamp,
  className
}: SystemBannerProps): React.ReactNode {
  const { t } = useTranslation()

  if (variant === 'iteration') {
    return (
      <TerminalDivider
        className={cn('my-1', className)}
        label={`iteration ${iteration}/${maxIterations || '∞'}`}
      />
    )
  }

  if (variant === 'handoff') {
    // Handoffs are flow plumbing; muted thin rule keeps them findable without dominating the stream.
    return <div className={cn('h-2 tty-divider-dashed opacity-50', className)} aria-hidden />
  }

  if (variant === 'mission') {
    return (
      <section
        className={cn(
          'group/mission relative w-full rounded-md border border-role-human bg-role-human',
          'px-4 py-3 font-mono',
          className
        )}
        aria-label={t('console.missionSpecs')}
      >
        <header className="flex items-baseline gap-2 text-[10px] uppercase tracking-[0.18em]">
          <span aria-hidden className="role-human select-none">
            ▶
          </span>
          <span className="role-human font-bold">{t('console.missionSpecs')}</span>
          {timestamp !== undefined && (
            <span className="text-muted-foreground-faint tabular-nums normal-case tracking-normal">
              {formatClock(timestamp)}
            </span>
          )}
        </header>
        {content && (
          <div className="mt-2 pl-[2ch] text-[13px] leading-relaxed text-foreground/95 [overflow-wrap:anywhere]">
            <MarkdownContent content={content} />
          </div>
        )}
      </section>
    )
  }

  // human-feedback
  return (
    <TerminalBlock
      role="human"
      state="done"
      glyph=">"
      label="HUMAN"
      timestamp={timestamp}
      className={className}
    >
      {content && (
        <div className="text-[12px] leading-relaxed">
          <MarkdownContent content={content} />
        </div>
      )}
    </TerminalBlock>
  )
}
