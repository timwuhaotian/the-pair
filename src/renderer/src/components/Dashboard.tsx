import React, { lazy, Suspense } from 'react'
import { useTranslation } from 'react-i18next'
import { usePairStore, Pair } from '../store/usePairStore'
import { DashboardInsightPanel } from './DashboardInsightPanel'
import { PairListSection } from './PairListSection'

const PairConsole = lazy(() => import('./PairConsole'))
const PairOperationsPanel = lazy(() => import('./PairOperationsPanel'))

interface DashboardProps {
  selectedPair: Pair | null
  selectedPairId: string | null
  onSelectPair: (p: Pair) => void
  onDeletePair: (pair: Pair) => void
  deletingPairId: string | null
  onCreatePair: () => void
  onPausePair: (pairId: string) => void
  onResumePair: (pairId: string) => void
  onPauseSelectedPair: () => Promise<void>
  onResumeSelectedPair: () => Promise<void>
  onRestoreTask: (spec: string, mentorModel: string, executorModel: string) => void
}

function ColumnFallback(): React.ReactNode {
  return <div className="h-full w-full bg-background" />
}

export function Dashboard({
  selectedPair,
  selectedPairId,
  onSelectPair,
  onDeletePair,
  deletingPairId,
  onCreatePair,
  onPausePair,
  onResumePair,
  onPauseSelectedPair,
  onResumeSelectedPair,
  onRestoreTask
}: DashboardProps): React.ReactNode {
  const { t } = useTranslation()
  const pairs = usePairStore((state) => state.pairs)

  return (
    <div className="relative h-full overflow-hidden bg-background">
      <div className="relative z-10 flex h-full overflow-hidden">
        <div className="flex w-full min-w-[240px] max-w-[320px] shrink-0 flex-col border-r border-border px-4 py-4">
          <PairListSection
            pairs={pairs}
            selectedPairId={selectedPairId}
            onSelectPair={onSelectPair}
            onPausePair={onPausePair}
            onResumePair={onResumePair}
            onDeletePair={onDeletePair}
            onCreatePair={onCreatePair}
            deletingPairId={deletingPairId}
          />
        </div>

        <div className="flex flex-1 min-w-0 flex-col overflow-hidden">
          {selectedPair ? (
            <Suspense fallback={<ColumnFallback />}>
              <PairConsole pair={selectedPair} />
            </Suspense>
          ) : (
            <div className="flex h-full flex-col items-center justify-center gap-2 font-mono text-[12px] text-muted-foreground-faint">
              <span aria-hidden className="select-none text-foreground/40">
                {'>_'}
              </span>
              <span className="uppercase tracking-[0.16em]">{t('dashboard.console.empty')}</span>
              <span className="text-[11px] text-muted-foreground-faint">
                {t('dashboard.console.emptyHint')}
              </span>
            </div>
          )}
        </div>

        <div className="hidden w-[320px] shrink-0 xl:flex xl:flex-col">
          {selectedPair ? (
            <Suspense fallback={<ColumnFallback />}>
              <PairOperationsPanel
                pair={selectedPair}
                onPause={onPauseSelectedPair}
                onResume={onResumeSelectedPair}
                onRestoreTask={onRestoreTask}
              />
            </Suspense>
          ) : (
            <DashboardInsightPanel pairs={pairs} />
          )}
        </div>
      </div>
    </div>
  )
}
