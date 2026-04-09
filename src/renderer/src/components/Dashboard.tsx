import React from 'react'
import { Trash2, RefreshCw } from 'lucide-react'
import { motion, AnimatePresence } from 'framer-motion'
import { cn } from '../lib/utils'
import { usePairStore, Pair } from '../store/usePairStore'
import { fadeInUp, staggerContainer } from '../lib/animations'
import { isPairActive } from '../lib/pairStatus'
import { DashboardEmptyState } from './DashboardEmptyState'
import { StatusBadge } from './StatusBadge'
import { GlassCard } from './ui/GlassCard'
import { GlassButton } from './ui/GlassButton'
import { ResourceMeter } from './ui/ResourceMeter'

export function Dashboard({
  onSelectPair,
  onDeletePair,
  deletingPairId,
  onCreatePair
}: {
  onSelectPair: (p: Pair) => void
  onDeletePair: (pair: Pair) => void
  deletingPairId: string | null
  onCreatePair: () => void
}): React.ReactNode {
  const pairs = usePairStore((state) => state.pairs)

  const getPairGlow = (status: string): 'blue' | 'purple' | 'green' | 'amber' | 'none' => {
    if (status === 'Mentoring') return 'blue'
    if (status === 'Executing') return 'purple'
    if (status === 'Reviewing') return 'amber'
    if (status === 'Awaiting Human Review') return 'amber'
    if (status === 'Paused') return 'amber'
    if (status === 'Finished') return 'green'
    return 'none'
  }

  return (
    <div className="relative h-full overflow-hidden">
      <div className="absolute inset-0 bg-gradient-to-br from-background via-muted/15 to-background" />
      <div className="absolute inset-0 bg-[radial-gradient(circle_at_top_left,rgba(59,130,246,0.14),transparent_32%),radial-gradient(circle_at_top_right,rgba(168,85,247,0.14),transparent_30%)]" />
      {pairs.length === 0 ? (
        <div className="relative z-10 flex h-full items-center justify-center p-8">
          <DashboardEmptyState onCreatePair={onCreatePair} />
        </div>
      ) : (
        <div className="relative z-10 flex h-full flex-col p-6">
          <div className="mb-6 flex items-end justify-between gap-6">
            <div>
              <h1 className="flex items-center gap-3 text-3xl font-semibold tracking-tight text-foreground">
                Pair Containers
                <span className="inline-flex items-center gap-1.5 rounded-full bg-primary/10 border border-primary/20 px-3 py-1 text-xs font-semibold text-primary shadow-sm">
                  <span className="relative flex h-2 w-2">
                    {pairs.length > 0 && (
                      <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-primary opacity-75" />
                    )}
                    <span className="relative inline-flex h-2 w-2 rounded-full bg-primary" />
                  </span>
                  {pairs.length}
                </span>
              </h1>
              <p className="mt-2 max-w-2xl text-sm leading-relaxed text-muted-foreground">
                Each pair keeps its workspace, defaults, and task history. Open one to continue
                work, queue a new task, or swap mentor and executor defaults without recreating the
                setup.
              </p>
            </div>
          </div>

          <motion.div
            className="grid flex-1 grid-cols-1 gap-4 overflow-y-auto scrollbar-thin md:grid-cols-2 xl:grid-cols-3"
            variants={staggerContainer}
            initial="hidden"
            animate="visible"
          >
            <AnimatePresence>
              {pairs.map((pair) => {
                return (
                  <motion.div
                    key={pair.id}
                    variants={fadeInUp}
                    initial="hidden"
                    animate="visible"
                    exit={{ opacity: 0, scale: 0.95, transition: { duration: 0.2 } }}
                    layout
                    className="group"
                  >
                    <GlassCard
                      hoverable
                      onClick={() => onSelectPair(pair)}
                      glow={getPairGlow(pair.status)}
                      className="flex min-h-[140px] flex-col gap-2.5 p-4"
                    >
                      <div className="flex items-start justify-between gap-2">
                        <div className="min-w-0 flex-1">
                          <h3 className="truncate text-sm font-semibold leading-tight text-foreground">
                            {pair.name}
                          </h3>
                          <p
                            className="mt-0.5 truncate font-mono text-xs text-muted-foreground"
                            title={pair.directory}
                          >
                            {pair.directory}
                          </p>
                        </div>
                        <div className="flex shrink-0 items-center gap-2">
                          <StatusBadge
                            status={pair.status}
                            stalled={
                              (pair.turn === 'mentor' && pair.mentorActivity.phase === 'stalled') ||
                              (pair.turn === 'executor' &&
                                pair.executorActivity.phase === 'stalled')
                            }
                          />
                          <GlassButton
                            variant="destructive"
                            size="sm"
                            onClick={(event) => {
                              event.stopPropagation()
                              onDeletePair(pair)
                            }}
                            disabled={deletingPairId === pair.id}
                            icon={<Trash2 size={12} />}
                            className="opacity-0 transition-opacity duration-200 group-hover:opacity-100"
                          >
                            {deletingPairId === pair.id ? 'Deleting...' : 'Delete'}
                          </GlassButton>
                        </div>
                      </div>

                      <div className="flex flex-wrap gap-1.5">
                        <span className="rounded-full border border-border bg-muted/40 px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-[0.16em] text-muted-foreground">
                          Run {pair.runCount}
                        </span>
                        <span className="rounded-full border border-border bg-muted/40 px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-[0.16em] text-muted-foreground">
                          {pair.runHistory.length} archived
                        </span>
                        {pair.status === 'Paused' ? (
                          <span className="rounded-full border border-slate-500/25 bg-slate-500/10 px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-[0.16em] text-slate-700 dark:text-slate-200">
                            Paused
                          </span>
                        ) : (
                          !isPairActive(pair.status) && (
                            <span className="rounded-full border border-green-500/20 bg-green-500/10 px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-[0.16em] text-green-700 dark:text-green-300">
                              Ready for new task
                            </span>
                          )
                        )}
                        {(pair.pendingMentorModel || pair.pendingExecutorModel) && (
                          <span className="rounded-full border border-amber-500/20 bg-amber-500/10 px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-[0.16em] text-amber-700 dark:text-amber-300">
                            Models queued
                          </span>
                        )}
                      </div>

                      <div className="flex-1 text-xs leading-snug text-muted-foreground line-clamp-2 whitespace-pre-wrap break-words [overflow-wrap:anywhere]">
                        {pair.spec}
                      </div>

                      <div className="mt-auto space-y-1.5 border-t border-border/40 pt-2.5">
                        {/* 资源监控 */}
                        <ResourceMeter cpu={pair.cpuUsage} mem={pair.memUsage} compact hideLabels />

                        {/* 状态和模型信息 */}
                        <div className="flex items-center justify-between text-[9px]">
                          <div className="flex items-center gap-1.5 font-mono text-muted-foreground">
                            <RefreshCw
                              size={10}
                              className={cn(
                                pair.status === 'Executing' || pair.status === 'Mentoring'
                                  ? 'animate-spin text-blue-500'
                                  : 'text-muted-foreground/50'
                              )}
                            />
                            <span>
                              {pair.currentTurnCard ? `turn ${pair.currentTurnCard.role}` : 'idle'}
                            </span>
                          </div>

                          <div className="flex items-center gap-3">
                            <div className="flex items-center gap-1">
                              <span className="font-bold text-blue-600">M</span>
                              <span className="max-w-[70px] truncate font-mono text-muted-foreground">
                                {(pair.pendingMentorModel ?? pair.mentorModel).split('/').pop()}
                              </span>
                            </div>
                            <div className="flex items-center gap-1">
                              <span className="font-bold text-purple-600">E</span>
                              <span className="max-w-[70px] truncate font-mono text-muted-foreground">
                                {(pair.pendingExecutorModel ?? pair.executorModel).split('/').pop()}
                              </span>
                            </div>
                          </div>
                        </div>
                      </div>
                    </GlassCard>
                  </motion.div>
                )
              })}
            </AnimatePresence>
          </motion.div>
        </div>
      )}
    </div>
  )
}
