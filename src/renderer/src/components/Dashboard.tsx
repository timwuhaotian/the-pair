import React, { lazy, Suspense } from 'react'
import { useTranslation } from 'react-i18next'
import { usePairStore, Pair } from '../store/usePairStore'
import { DashboardInsightPanel } from './DashboardInsightPanel'
import { PairListSection } from './PairListSection'
import { StartupHero } from './StartupHero'

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
            <EmptyPairConsole pairCount={pairs.length} onCreatePair={onCreatePair} />
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

function EmptyPairConsole({
  pairCount,
  onCreatePair
}: {
  pairCount: number
  onCreatePair: () => void
}): React.ReactNode {
  const { t } = useTranslation()
  const isFirstRun = pairCount === 0

  return (
    <div className="flex h-full items-center justify-center overflow-y-auto px-6 py-8 font-mono">
      <div className="flex w-full max-w-[760px] flex-col items-center gap-8 md:flex-row md:items-center md:justify-center md:gap-10">
        {/* left — brand hero artwork */}
        <div className="shrink-0">
          <StartupHero
            size="md"
            animated
            tagline={t('startup.tagline')}
            wordmark={t('startup.wordmark')}
            caption={isFirstRun ? t('startup.firstRunCaption') : undefined}
          />
        </div>

        {/* right — terminal-style explainer */}
        <div className="flex w-full max-w-[320px] flex-col gap-3 text-[12px] md:border-l md:border-border md:pl-10">
          <div className="flex items-baseline gap-2 text-[10px] uppercase tracking-[0.22em] text-muted-foreground-faint">
            <span aria-hidden>{'>_'}</span>
            <span>{t('startup.consoleEmptyTitle').toLowerCase()}</span>
          </div>

          <div className="flex flex-col gap-1.5 border border-border bg-background/40 p-3 text-[11px] leading-relaxed">
            <div className="flex items-baseline gap-2">
              <span aria-hidden className="role-mentor select-none">
                ●
              </span>
              <span className="role-mentor font-bold uppercase tracking-[0.14em]">
                {t('common.mentor').toLowerCase()}
              </span>
              <span className="text-muted-foreground">· {t('emptyState.mentorDesc')}</span>
            </div>
            <div className="flex items-baseline gap-2">
              <span aria-hidden className="text-muted-foreground select-none">
                ⇄
              </span>
              <span className="text-foreground/85 font-bold uppercase tracking-[0.14em]">
                {t('common.handoff').toLowerCase()}
              </span>
              <span className="text-muted-foreground">· {t('emptyState.handoffDesc')}</span>
            </div>
            <div className="flex items-baseline gap-2">
              <span aria-hidden className="role-executor select-none">
                ●
              </span>
              <span className="role-executor font-bold uppercase tracking-[0.14em]">
                {t('common.executor').toLowerCase()}
              </span>
              <span className="text-muted-foreground">· {t('emptyState.executorDesc')}</span>
            </div>
          </div>

          <p className="text-[11px] leading-relaxed text-muted-foreground">
            {isFirstRun ? t('startup.firstRunHint') : t('dashboard.console.emptyHint')}
          </p>

          {isFirstRun && (
            <button
              type="button"
              onClick={onCreatePair}
              className="mt-1 inline-flex items-center justify-center gap-2 self-start rounded-sm border border-foreground bg-foreground px-4 py-2 text-[11px] font-bold uppercase tracking-[0.14em] text-background transition-colors duration-150 hover:bg-foreground/90 cursor-pointer"
            >
              <span aria-hidden>▸</span>
              {t('emptyState.createFirst').toLowerCase()}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
