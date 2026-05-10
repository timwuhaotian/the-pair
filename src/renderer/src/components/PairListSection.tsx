import { useTranslation } from 'react-i18next'
import { Plus } from 'lucide-react'
import { type Pair } from '../store/usePairStore'
import { buildPairGroups } from '../lib/dashboardPairs'
import { PairListGroup } from './PairListGroup'

interface PairListSectionProps {
  pairs: Pair[]
  onSelectPair: (pair: Pair) => void
  onPausePair: (pairId: string) => void
  onResumePair: (pairId: string) => void
  onDeletePair: (pair: Pair) => void
  onCreatePair: () => void
  deletingPairId: string | null
}

export function PairListSection({
  pairs,
  onSelectPair,
  onPausePair,
  onResumePair,
  onDeletePair,
  onCreatePair,
  deletingPairId
}: PairListSectionProps): React.ReactNode {
  const { t } = useTranslation()
  const pairGroups = buildPairGroups(pairs)
  const createButton = (
    <button
      type="button"
      onClick={onCreatePair}
      className="inline-flex items-center justify-center gap-2 rounded-lg border border-primary/20 bg-primary px-4 py-2.5 text-sm font-semibold text-primary-foreground shadow-lg transition-colors hover:bg-primary/90"
    >
      <Plus size={16} />
      {t('quickActions.create')}
    </button>
  )

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <h2 className="mb-4 text-sm font-semibold text-foreground">{t('dashboard.pairs.title')}</h2>

      <div className="flex-1 space-y-4 overflow-y-auto scrollbar-thin pr-2">
        {pairGroups.length === 0 ? (
          <div className="flex min-h-[320px] items-center justify-center">{createButton}</div>
        ) : (
          <>
            {pairGroups.map((group) => (
              <PairListGroup
                key={group.key}
                title={t(group.titleKey)}
                pairs={group.pairs}
                onSelectPair={onSelectPair}
                onPausePair={onPausePair}
                onResumePair={onResumePair}
                onDeletePair={onDeletePair}
                deletingPairId={deletingPairId}
              />
            ))}
            <div className="flex justify-center pt-2">{createButton}</div>
          </>
        )}
      </div>
    </div>
  )
}
