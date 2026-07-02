import React, { useMemo, useState } from 'react'
import { SlidersHorizontal } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { usePairStore, Pair } from '../store/usePairStore'
import type { PairModelSelection } from '../types'
import { GlassButton } from './ui/GlassButton'
import { GlassModal } from './ui/GlassModal'
import { ModelPicker } from './ModelPicker'
import { isPairActive } from '../lib/pairStatus'

interface PairSettingsModalProps {
  pair: Pair | null
  isOpen: boolean
  onClose: () => void
}

export function PairSettingsModal({
  pair,
  isOpen,
  onClose
}: PairSettingsModalProps): React.ReactNode {
  const { t } = useTranslation()
  const availableModels = usePairStore((s) => s.availableModels)
  const updatePairModels = usePairStore((s) => s.updatePairModels)
  const isLoading = usePairStore((s) => s.isLoading)
  const error = usePairStore((s) => s.error)
  const [selection, setSelection] = useState<PairModelSelection>(() => ({
    mentorModel: pair?.pendingMentorModel ?? pair?.mentorModel ?? '',
    executorModel: pair?.pendingExecutorModel ?? pair?.executorModel ?? '',
    mentorReasoningEffort: pair?.mentorReasoningEffort,
    executorReasoningEffort: pair?.executorReasoningEffort
  }))

  const queuedForNextTask = useMemo(
    () => Boolean(pair?.pendingMentorModel || pair?.pendingExecutorModel),
    [pair]
  )

  if (!pair) return null

  const handleSubmit = async (e: React.FormEvent): Promise<void> => {
    e.preventDefault()

    try {
      await updatePairModels(pair.id, selection)
      onClose()
    } catch {
      // Store already holds the user-facing error
    }
  }

  return (
    <GlassModal
      isOpen={isOpen}
      onClose={onClose}
      title={t('modals.pairDefaults', { name: pair.name })}
      className="max-w-3xl"
    >
      <form onSubmit={handleSubmit} className="flex flex-col gap-3 font-mono text-[12px]">
        <div className="border-l-2 border-border bg-background/40 px-3 py-2">
          <div className="flex items-baseline gap-2">
            <SlidersHorizontal size={11} className="text-muted-foreground translate-y-px" />
            <div className="space-y-0.5">
              <div className="text-foreground/90 text-[12px]">{t('modals.pairDefaultsDesc')}</div>
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                {t('modals.pairDefaultsNote')}
              </p>
            </div>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-3">
          <ModelPicker
            value={selection.mentorModel}
            models={availableModels}
            onChange={(mentorModel) => setSelection((current) => ({ ...current, mentorModel }))}
            role="mentor"
            variant="card"
            reasoningEffort={selection.mentorReasoningEffort}
            onReasoningEffortChange={(mentorReasoningEffort) =>
              setSelection((current) => ({ ...current, mentorReasoningEffort }))
            }
          />
          <ModelPicker
            value={selection.executorModel}
            models={availableModels}
            onChange={(executorModel) => setSelection((current) => ({ ...current, executorModel }))}
            role="executor"
            variant="card"
            reasoningEffort={selection.executorReasoningEffort}
            onReasoningEffortChange={(executorReasoningEffort) =>
              setSelection((current) => ({ ...current, executorReasoningEffort }))
            }
          />
        </div>

        {(isPairActive(pair.status) || queuedForNextTask) && (
          <div className="border-l-2 border-state-running bg-state-running/10 px-3 py-2 text-[11px] state-running">
            ! {queuedForNextTask ? t('modals.modelUpdateQueued') : t('modals.pairRunningNote')}
          </div>
        )}

        {error && (
          <div className="border-l-2 border-state-error bg-state-error/10 px-3 py-2 text-[11px] state-error">
            ✗ {error}
          </div>
        )}

        <div className="flex justify-end gap-2 pt-2 border-t border-border">
          <GlassButton
            type="button"
            variant="ghost"
            onClick={onClose}
            data-testid="settings-cancel-btn"
          >
            {t('common.cancel')}
          </GlassButton>
          <GlassButton
            type="submit"
            variant="primary"
            disabled={isLoading || !selection.mentorModel || !selection.executorModel}
            data-testid="settings-save-btn"
          >
            {isLoading ? t('common.saving') : `▸ ${t('modals.saveDefaults').toLowerCase()}`}
          </GlassButton>
        </div>
      </form>
    </GlassModal>
  )
}
