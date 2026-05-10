import { motion } from 'framer-motion'
import { Cpu, MemoryStick, Pause, Play, Trash2, GitBranch } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { type Pair } from '../store/usePairStore'
import { StatusBadge } from './StatusBadge'
import { isPairActive } from '../lib/pairStatus'
import { fadeInUp } from '../lib/animations'

interface PairListGroupProps {
  title: string
  pairs: Pair[]
  onSelectPair: (pair: Pair) => void
  onPausePair: (pairId: string) => void
  onResumePair: (pairId: string) => void
  onDeletePair: (pair: Pair) => void
  deletingPairId: string | null
  className?: string
}

export function PairListGroup({
  title,
  pairs,
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
    <div className={className}>
      <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
        {title}
      </h3>
      <div className="grid grid-cols-1 gap-3 2xl:grid-cols-2">
        {pairs.map((pair) => (
          <motion.div
            key={pair.id}
            variants={fadeInUp}
            initial="hidden"
            animate="visible"
            className="group glass-card glass-card-hover cursor-pointer rounded-lg border border-border/60 px-4 py-3"
            onClick={() => onSelectPair(pair)}
          >
            <div className="flex h-full flex-col gap-3">
              <div className="flex items-start gap-3">
                <StatusBadge
                  status={pair.status}
                  stalled={
                    (pair.turn === 'mentor' && pair.mentorActivity.phase === 'stalled') ||
                    (pair.turn === 'executor' && pair.executorActivity.phase === 'stalled')
                  }
                />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-semibold text-foreground">{pair.name}</div>
                  <div
                    className="truncate text-[11px] font-mono text-muted-foreground"
                    title={pair.directory}
                  >
                    {pair.directory}
                  </div>
                </div>

                <div className="flex shrink-0 items-center gap-1 opacity-100 transition-opacity sm:opacity-0 sm:group-hover:opacity-100">
                  {pair.status === 'Paused' ? (
                    <button
                      aria-label="Resume pair"
                      onClick={(e) => {
                        e.stopPropagation()
                        onResumePair(pair.id)
                      }}
                      className="rounded-md p-1.5 text-muted-foreground hover:bg-muted/60 hover:text-foreground"
                    >
                      <Play size={14} />
                    </button>
                  ) : isPairActive(pair.status) ? (
                    <button
                      aria-label="Pause pair"
                      onClick={(e) => {
                        e.stopPropagation()
                        onPausePair(pair.id)
                      }}
                      className="rounded-md p-1.5 text-muted-foreground hover:bg-muted/60 hover:text-foreground"
                    >
                      <Pause size={14} />
                    </button>
                  ) : null}
                  <button
                    aria-label="Delete pair"
                    onClick={(e) => {
                      e.stopPropagation()
                      onDeletePair(pair)
                    }}
                    disabled={deletingPairId === pair.id}
                    className="rounded-md p-1.5 text-muted-foreground hover:bg-destructive/10 hover:text-destructive disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>

              <div className="grid grid-cols-4 gap-2 text-[11px] text-muted-foreground">
                <div className="rounded-md bg-muted/40 px-2 py-1.5">
                  <div className="font-medium text-foreground tabular-nums">
                    {pair.iterations}/{pair.maxIterations}
                  </div>
                  <div>{t('dashboard.card.turns')}</div>
                </div>
                <div className="rounded-md bg-muted/40 px-2 py-1.5">
                  <div className="flex items-center gap-1 font-medium text-foreground tabular-nums">
                    <Cpu size={12} />
                    {pair.cpuUsage.toFixed(1)}%
                  </div>
                  <div>{t('dashboard.card.cpu')}</div>
                </div>
                <div className="rounded-md bg-muted/40 px-2 py-1.5">
                  <div className="flex items-center gap-1 font-medium text-foreground tabular-nums">
                    <MemoryStick size={12} />
                    {Math.round(pair.memUsage)}
                  </div>
                  <div>{t('dashboard.card.memory')}</div>
                </div>
                <div className="rounded-md bg-muted/40 px-2 py-1.5">
                  <div className="flex items-center gap-1 font-medium text-foreground tabular-nums">
                    <GitBranch size={12} />
                    {pair.modifiedFiles.length}
                  </div>
                  <div>{t('dashboard.card.files')}</div>
                </div>
              </div>
            </div>
          </motion.div>
        ))}
      </div>
    </div>
  )
}
