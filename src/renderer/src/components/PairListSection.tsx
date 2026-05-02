import { useTranslation } from 'react-i18next'
import { type Pair } from '../store/usePairStore'
import { isPairActive } from '../lib/pairStatus'
import { PairListGroup } from './PairListGroup'

interface PairListSectionProps {
  pairs: Pair[]
  onSelectPair: (pair: Pair) => void
  onPausePair: (pairId: string) => void
  onResumePair: (pairId: string) => void
  onDeletePair: (pair: Pair) => void
  deletingPairId: string | null
}

export function PairListSection({
  pairs,
  onSelectPair,
  onPausePair,
  onResumePair,
  onDeletePair,
  deletingPairId
}: PairListSectionProps): React.ReactNode {
  const { t } = useTranslation()

  const runningPairs = pairs.filter((p) => isPairActive(p.status))
  const pausedPairs = pairs.filter((p) => p.status === 'Paused')
  const idlePairs = pairs.filter((p) => p.status === 'Idle' || p.status === 'Finished')

  const hasActivePairs = runningPairs.length > 0 || pausedPairs.length > 0

  return (
    <div className="flex flex-col overflow-hidden">
      <h2 className="mb-4 text-sm font-semibold text-foreground">{t('dashboard.pairs.title')}</h2>

      <div className="flex-1 space-y-4 overflow-y-auto scrollbar-thin pr-2">
        {hasActivePairs && (
          <PairListGroup
            title={t('dashboard.groups.active')}
            pairs={runningPairs}
            onSelectPair={onSelectPair}
            onPausePair={onPausePair}
            onResumePair={onResumePair}
            onDeletePair={onDeletePair}
            deletingPairId={deletingPairId}
          />
        )}

        {pausedPairs.length > 0 && (
          <PairListGroup
            title={t('dashboard.groups.paused')}
            pairs={pausedPairs}
            onSelectPair={onSelectPair}
            onPausePair={onPausePair}
            onResumePair={onResumePair}
            onDeletePair={onDeletePair}
            deletingPairId={deletingPairId}
          />
        )}

        {idlePairs.length > 0 && (
          <PairListGroup
            title={t('dashboard.groups.completed')}
            pairs={idlePairs}
            onSelectPair={onSelectPair}
            onPausePair={onPausePair}
            onResumePair={onResumePair}
            onDeletePair={onDeletePair}
            deletingPairId={deletingPairId}
          />
        )}
      </div>
    </div>
  )
}
