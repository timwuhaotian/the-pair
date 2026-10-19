import { motion } from 'framer-motion'
import { Pause, Play, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { type Pair } from '../store/usePairStore'
import { isPairActive } from '../lib/pairStatus'
import { fadeInUp } from '../lib/animations'
import { cn, formatIterations } from '../lib/utils'
import { activeTint, hoverTint, sectionLabel } from '../lib/ui'
import { IconButton } from './ui/IconButton'

interface PairListGroupProps {
  title: string
  pairs: Pair[]
  selectedPairId?: string | null
  onSelectPair: (pair: Pair) => void
  onPausePair: (pairId: string) => void
  onResumePair: (pairId: string) => void
  onDeletePair: (pair: Pair) => void
  deletingPairId: string | null
  className?: string
}

function statusGlyph(pair: Pair): { glyph: string; tone: string } {
  if (pair.status === 'Error') return { glyph: '✗', tone: 'state-error' }
  if (pair.status === 'Finished') return { glyph: '✓', tone: 'state-done' }
  if (pair.status === 'Paused' || pair.status === 'Awaiting Human Review')
    return { glyph: '◌', tone: 'state-running' }
  if (isPairActive(pair.status)) return { glyph: '●', tone: 'state-done' }
  return { glyph: '○', tone: 'text-muted-foreground' }
}

export function PairListGroup({
  title,
  pairs,
  selectedPairId,
  onSelectPair,
  onPausePair,
  onResumePair,
  onDeletePair,
  deletingPairId,
  className
}: PairListGroupProps): React.ReactNode {
  const { t } = useTranslation()

  if (pairs.length === 0) return null

  return (
    <div className={cn('space-y-1.5 pl-2', className)}>
      <h3 className={cn(sectionLabel, 'pl-1')}>
        {title} <span className="tabular-nums text-muted-foreground-faint">({pairs.length})</span>
      </h3>
      <div className="space-y-1">
        {pairs.map((pair) => {
          const { glyph, tone } = statusGlyph(pair)
          const stalled =
            (pair.turn === 'mentor' && pair.mentorActivity.phase === 'stalled') ||
            (pair.turn === 'executor' && pair.executorActivity.phase === 'stalled')
          const isSelected = selectedPairId === pair.id

          return (
            <motion.div
              key={pair.id}
              variants={fadeInUp}
              initial="hidden"
              animate="visible"
              className={cn(
                'group relative flex w-full items-center rounded-sm border pr-1.5 font-mono text-[12px] transition-colors',
                isSelected
                  ? cn('border-foreground/25 text-foreground', activeTint)
                  : cn('border-border/45 bg-foreground/[0.015] hover:border-border', hoverTint)
              )}
            >
              {isSelected && (
                <span
                  aria-hidden
                  className="absolute inset-y-1 left-0 w-[2px] rounded-full bg-foreground/70"
                />
              )}
              <button
                type="button"
                onClick={() => onSelectPair(pair)}
                aria-current={isSelected ? 'true' : undefined}
                className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 py-2 pl-2.5 pr-1 text-left"
              >
                <span
                  aria-hidden
                  className={cn(
                    'shrink-0 w-[1ch] tabular-nums select-none text-[13px] leading-none',
                    tone,
                    stalled && 'state-error tty-blink'
                  )}
                  title={pair.status}
                >
                  {stalled ? '!' : glyph}
                </span>

                <span
                  className={cn(
                    'min-w-0 flex-1 truncate',
                    isSelected ? 'text-foreground' : 'text-foreground/90'
                  )}
                >
                  {pair.name}
                </span>

                <span className="shrink-0 tabular-nums text-[10px] text-muted-foreground-faint">
                  {formatIterations(pair.iterations, pair.maxIterations)}
                </span>
              </button>

              <span className="flex shrink-0 items-center opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
                {pair.status === 'Paused' ? (
                  <IconButton
                    aria-label={t('common.resume', { defaultValue: 'Resume' })}
                    onClick={() => onResumePair(pair.id)}
                  >
                    <Play size={11} />
                  </IconButton>
                ) : isPairActive(pair.status) ? (
                  <IconButton
                    aria-label={t('common.pause', { defaultValue: 'Pause' })}
                    onClick={() => onPausePair(pair.id)}
                  >
                    <Pause size={11} />
                  </IconButton>
                ) : null}
                <IconButton
                  aria-label={t('common.delete', { defaultValue: 'Delete' })}
                  onClick={() => onDeletePair(pair)}
                  disabled={deletingPairId === pair.id}
                  destructive
                  data-testid={`pair-card-delete-${pair.id}`}
                >
                  <Trash2 size={11} />
                </IconButton>
              </span>
            </motion.div>
          )
        })}
      </div>
    </div>
  )
}
