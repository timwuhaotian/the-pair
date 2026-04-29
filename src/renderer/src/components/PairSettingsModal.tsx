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
  const { availableModels, updatePairModels, isLoading, error } = usePairStore()
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
      <form onSubmit={handleSubmit} className="flex flex-col gap-4">
        <div className="glass-card rounded-xl p-3">
          <div className="flex items-start gap-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-xl border border-border bg-muted/60">
              <SlidersHorizontal size={14} className="text-foreground/70" />
            </div>
            <div className="space-y-1">
              <div className="text-sm font-semibold text-foreground">
                {t('modals.pairDefaultsDesc')}
              </div>
              <p className="text-sm leading-relaxed text-muted-foreground">
                {t('modals.pairDefaultsNote')}
              </p>
            </div>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-4">
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
          <div className="rounded-xl border border-amber-500/20 bg-amber-500/10 px-3.5 py-2.5 text-sm text-amber-700 dark:text-amber-300">
            {queuedForNextTask ? t('modals.modelUpdateQueued') : t('modals.pairRunningNote')}
          </div>
        )}

        {error && (
          <div className="rounded-xl border border-destructive/20 bg-destructive/10 px-3.5 py-2.5 text-sm text-destructive">
            {error}
          </div>
        )}

        <div className="flex justify-end gap-3">
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
            {isLoading ? t('common.saving') : t('modals.saveDefaults')}
          </GlassButton>
        </div>
      </form>
    </GlassModal>
  )
}
