import { motion } from 'framer-motion'
import { Pause, Play, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { type Pair } from '../store/usePairStore'
import { isPairActive } from '../lib/pairStatus'
import { fadeInUp } from '../lib/animations'
import { cn, formatIterations } from '../lib/utils'

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
      <h3 className="font-mono text-[10px] uppercase tracking-[0.18em] text-muted-foreground-faint pl-1">
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
            <motion.button
              key={pair.id}
              type="button"
              variants={fadeInUp}
              initial="hidden"
              animate="visible"
              onClick={() => onSelectPair(pair)}
              className={cn(
                'group relative flex w-full items-center gap-2 px-2.5 py-2 text-left font-mono text-[12px] rounded-sm border transition-colors cursor-pointer',
                isSelected
                  ? 'bg-foreground/[0.07] text-foreground border-foreground/25'
                  : 'border-border/45 bg-foreground/[0.015] hover:bg-foreground/[0.05] hover:border-border'
              )}
              aria-current={isSelected ? 'true' : undefined}
            >
              {isSelected && (
                <span
                  aria-hidden
                  className="absolute inset-y-1 left-0 w-[2px] rounded-full bg-foreground/70"
                />
              )}
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

              <span className="flex shrink-0 items-center gap-0 opacity-0 group-hover:opacity-100 transition-opacity">
                {pair.status === 'Paused' ? (
                  <span
                    role="button"
                    aria-label={t('common.resume', { defaultValue: 'Resume' })}
                    onClick={(e) => {
                      e.stopPropagation()
                      onResumePair(pair.id)
                    }}
                    className="p-1 text-muted-foreground hover:text-foreground cursor-pointer"
                  >
                    <Play size={11} />
                  </span>
                ) : isPairActive(pair.status) ? (
                  <span
                    role="button"
                    aria-label={t('common.pause', { defaultValue: 'Pause' })}
                    onClick={(e) => {
                      e.stopPropagation()
                      onPausePair(pair.id)
                    }}
                    className="p-1 text-muted-foreground hover:text-foreground cursor-pointer"
                  >
                    <Pause size={11} />
                  </span>
                ) : null}
                <span
                  role="button"
                  aria-label={t('common.delete', { defaultValue: 'Delete' })}
                  onClick={(e) => {
                    e.stopPropagation()
                    onDeletePair(pair)
                  }}
                  data-testid={`pair-card-delete-${pair.id}`}
                  className={cn(
                    'p-1 text-muted-foreground hover:text-[var(--state-error)] cursor-pointer',
                    deletingPairId === pair.id && 'opacity-40 pointer-events-none'
                  )}
                >
                  <Trash2 size={11} />
                </span>
              </span>
            </motion.button>
          )
        })}
      </div>
    </div>
  )
}
