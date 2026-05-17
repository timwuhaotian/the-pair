import { useTranslation } from 'react-i18next'
import { Folder, Plus } from 'lucide-react'
import { type Pair } from '../store/usePairStore'
import { buildWorkspaceGroups } from '../lib/dashboardPairs'
import { PairListGroup } from './PairListGroup'
import { GlassButton } from './ui/GlassButton'

interface PairListSectionProps {
  pairs: Pair[]
  selectedPairId: string | null
  onSelectPair: (pair: Pair) => void
  onPausePair: (pairId: string) => void
  onResumePair: (pairId: string) => void
  onDeletePair: (pair: Pair) => void
  onCreatePair: () => void
  deletingPairId: string | null
}

export function PairListSection({
  pairs,
  selectedPairId,
  onSelectPair,
  onPausePair,
  onResumePair,
  onDeletePair,
  onCreatePair,
  deletingPairId
}: PairListSectionProps): React.ReactNode {
  const { t } = useTranslation()
  const workspaceGroups = buildWorkspaceGroups(pairs)

  const createButton = (
    <GlassButton variant="primary" size="sm" icon={<Plus size={12} />} onClick={onCreatePair}>
      {t('quickActions.create')}
    </GlassButton>
  )

  return (
    <div className="flex h-full flex-col overflow-hidden font-mono">
      <div className="mb-3 flex items-center justify-between gap-2 shrink-0">
        <h2 className="text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
          {t('dashboard.pairs.title')}{' '}
          <span className="tabular-nums text-muted-foreground-faint">({pairs.length})</span>
        </h2>
        {workspaceGroups.length > 0 && createButton}
      </div>

      <div className="flex-1 space-y-5 overflow-y-auto scrollbar-thin pr-2">
        {workspaceGroups.length === 0 ? (
          <div className="flex min-h-[280px] flex-col items-start justify-center gap-3 px-2">
            <div className="text-[12px] uppercase tracking-[0.14em] text-muted-foreground-faint">
              — no pairs yet —
            </div>
            <div className="text-[11px] text-muted-foreground">{'>'} run a new pair to begin</div>
            {createButton}
          </div>
        ) : (
          workspaceGroups.map((workspace) => (
            <section key={workspace.directory} className="space-y-2">
              <header
                className="flex items-baseline justify-between gap-2 px-1 pb-1 border-b border-border/40"
                title={workspace.directory}
              >
                <div className="flex items-baseline gap-1.5 min-w-0">
                  <Folder
                    size={10}
                    className="shrink-0 translate-y-px text-muted-foreground-faint"
                  />
                  <span className="truncate text-[11px] uppercase tracking-[0.16em] text-foreground/85">
                    {workspace.shortName}
                  </span>
                  <span className="shrink-0 tabular-nums text-[10px] text-muted-foreground-faint">
                    ({workspace.pairs.length})
                  </span>
                </div>
                <span className="hidden lg:inline truncate text-[10px] text-muted-foreground-faint max-w-[18ch]">
                  {workspace.directory}
                </span>
              </header>
              {workspace.statusGroups.map((group) => (
                <PairListGroup
                  key={`${workspace.directory}::${group.key}`}
                  title={t(group.titleKey)}
                  pairs={group.pairs}
                  selectedPairId={selectedPairId}
                  onSelectPair={onSelectPair}
                  onPausePair={onPausePair}
                  onResumePair={onResumePair}
                  onDeletePair={onDeletePair}
                  deletingPairId={deletingPairId}
                />
              ))}
            </section>
          ))
        )}
      </div>
    </div>
  )
}
