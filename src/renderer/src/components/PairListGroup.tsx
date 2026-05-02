import { motion } from 'framer-motion'
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
  if (pairs.length === 0) return null

  return (
    <div className={className}>
      <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
        {title}
      </h3>
      <div className="space-y-1">
        {pairs.map((pair) => (
          <motion.div
            key={pair.id}
            variants={fadeInUp}
            initial="hidden"
            animate="visible"
            className="group flex cursor-pointer items-center gap-3 rounded-lg border border-transparent px-3 py-2.5 hover:border-border/50 hover:bg-muted/30"
            onClick={() => onSelectPair(pair)}
          >
            <div className="flex min-w-0 flex-1 items-center gap-2">
              <StatusBadge
                status={pair.status}
                stalled={
                  (pair.turn === 'mentor' && pair.mentorActivity.phase === 'stalled') ||
                  (pair.turn === 'executor' && pair.executorActivity.phase === 'stalled')
                }
              />
              <div className="min-w-0">
                <div className="truncate text-sm font-medium text-foreground">{pair.name}</div>
                <div
                  className="truncate text-[11px] font-mono text-muted-foreground"
                  title={pair.directory}
                >
                  {pair.directory}
                </div>
              </div>
            </div>

            <div className="shrink-0 text-xs text-muted-foreground">
              {pair.iterations}/{pair.maxIterations}
            </div>

            <div className="hidden shrink-0 items-center gap-1 group-hover:flex">
              {pair.status === 'Paused' ? (
                <button
                  onClick={(e) => {
                    e.stopPropagation()
                    onResumePair(pair.id)
                  }}
                  className="rounded p-1 text-muted-foreground hover:bg-muted/50 hover:text-foreground"
                >
                  ▶
                </button>
              ) : isPairActive(pair.status) ? (
                <button
                  onClick={(e) => {
                    e.stopPropagation()
                    onPausePair(pair.id)
                  }}
                  className="rounded p-1 text-muted-foreground hover:bg-muted/50 hover:text-foreground"
                >
                  ⏸
                </button>
              ) : null}
              <button
                onClick={(e) => {
                  e.stopPropagation()
                  onDeletePair(pair)
                }}
                disabled={deletingPairId === pair.id}
                className="rounded p-1 text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
              >
                ✕
              </button>
            </div>
          </motion.div>
        ))}
      </div>
    </div>
  )
}
