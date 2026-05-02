import React from 'react'
import { usePairStore, Pair } from '../store/usePairStore'
import { isPairActive } from '../lib/pairStatus'
import { StatCards } from './StatCards'
import { EmptyStateGuide } from './EmptyStateGuide'
import { RecentActivityPanel } from './RecentActivityPanel'
import { PairListSection } from './PairListSection'

export function Dashboard({
  onSelectPair,
  onDeletePair,
  deletingPairId,
  onCreatePair,
  onPausePair,
  onResumePair
}: {
  onSelectPair: (p: Pair) => void
  onDeletePair: (pair: Pair) => void
  deletingPairId: string | null
  onCreatePair: () => void
  onPausePair: (pairId: string) => void
  onResumePair: (pairId: string) => void
}): React.ReactNode {
  const pairs = usePairStore((state) => state.pairs)

  const stats = {
    total: pairs.length,
    running: pairs.filter((p) => isPairActive(p.status)).length,
    paused: pairs.filter((p) => p.status === 'Paused').length,
    finished: pairs.filter((p) => p.status === 'Finished').length
  }

  return (
    <div className="relative h-full overflow-hidden">
      <div className="absolute inset-0 bg-gradient-to-br from-background via-muted/15 to-background" />
      <div className="absolute inset-0 bg-[radial-gradient(circle_at_top_left,rgba(59,130,246,0.14),transparent_32%),radial-gradient(circle_at_top_right,rgba(168,85,247,0.14),transparent_30%)]" />

      <div className="relative z-10 flex h-full flex-col p-6">
        {/* Stat Cards - always visible */}
        <StatCards
          total={stats.total}
          running={stats.running}
          paused={stats.paused}
          finished={stats.finished}
          className="mb-6"
        />

        {/* Empty state or list + activity */}
        {pairs.length === 0 ? (
          <div className="flex flex-1 items-center justify-center">
            <EmptyStateGuide onCreatePair={onCreatePair} />
          </div>
        ) : (
          <div className="flex flex-1 gap-6 overflow-hidden">
            <div className="w-full overflow-hidden xl:w-2/3">
              <PairListSection
                pairs={pairs}
                onSelectPair={onSelectPair}
                onPausePair={onPausePair}
                onResumePair={onResumePair}
                onDeletePair={onDeletePair}
                deletingPairId={deletingPairId}
              />
            </div>
            <div className="hidden w-1/3 overflow-hidden xl:block">
              <RecentActivityPanel />
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
