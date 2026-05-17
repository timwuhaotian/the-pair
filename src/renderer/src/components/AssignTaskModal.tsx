import React, { useMemo, useState, useRef, useCallback } from 'react'
import { ArrowUpRight, Sparkles, RotateCcw } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { usePairStore, Pair } from '../store/usePairStore'
import { GlassButton } from './ui/GlassButton'
import { GlassModal } from './ui/GlassModal'
import { FileMention } from './FileMention'
import { ModelPicker } from './ModelPicker'
import { SkillPicker } from './SkillPicker'
import { PresetPicker } from './PresetPicker'
import { usePresets } from '../lib/usePresets'
import { buildSpecFromPreset, stripTemplate } from '../lib/presetUtils'
import { getAssignableTaskModels } from '../lib/modelResolution'
import type { PairPreset } from '../types'

interface AssignTaskModalProps {
  pair: Pair | null
  isOpen: boolean
  onClose: () => void
}

export function AssignTaskModal({ pair, isOpen, onClose }: AssignTaskModalProps): React.ReactNode {
  const { t } = useTranslation()
  const { assignTask, isLoading, error, availableModels, restoringSpec, setRestoringSpec } =
    usePairStore()
  const [spec, setSpec] = useState('')
  const [fileContexts, setFileContexts] = useState<Map<string, string>>(new Map())
  const [selectedPreset, setSelectedPreset] = useState<PairPreset | null>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const {
    presets,
    loading: presetsLoading,
    error: presetsError,
    reload: loadPresets
  } = usePresets()

  const taskModelDefaults = useMemo(
    () =>
      pair
        ? getAssignableTaskModels(pair, restoringSpec ?? undefined)
        : { mentorModel: '', executorModel: '' },
    [pair, restoringSpec]
  )

  const restoringKey = restoringSpec
    ? `${restoringSpec.spec}:${restoringSpec.mentorModel}:${restoringSpec.executorModel}`
    : 'new'
  const modelDraftKey = `${isOpen ? 'open' : 'closed'}:${pair?.id ?? 'none'}:${restoringKey}:${taskModelDefaults.mentorModel}:${taskModelDefaults.executorModel}`
  const [modelDraft, setModelDraft] = useState(() => ({
    key: modelDraftKey,
    mentorModel: taskModelDefaults.mentorModel,
    executorModel: taskModelDefaults.executorModel
  }))

  const activeModelDraft = useMemo(
    () =>
      modelDraft.key === modelDraftKey
        ? modelDraft
        : {
            key: modelDraftKey,
            mentorModel: taskModelDefaults.mentorModel,
            executorModel: taskModelDefaults.executorModel
          },
    [modelDraft, modelDraftKey, taskModelDefaults]
  )

  const tempMentorModel = activeModelDraft.mentorModel
  const tempExecutorModel = activeModelDraft.executorModel
  const setTempMentorModel = useCallback(
    (mentorModel: string) => {
      setModelDraft({
        ...activeModelDraft,
        mentorModel
      })
    },
    [activeModelDraft]
  )
  const setTempExecutorModel = useCallback(
    (executorModel: string) => {
      setModelDraft({
        ...activeModelDraft,
        executorModel
      })
    },
    [activeModelDraft]
  )

  const isRestoring = !!restoringSpec

  const handleFileSelect = useCallback((path: string, content: string): void => {
    setFileContexts((prev) => {
      const next = new Map(prev)
      next.set(path, content)
      return next
    })
  }, [])

  const handlePresetSelect = useCallback((preset: PairPreset | null) => {
    setSelectedPreset(preset)
    if (preset) {
      setSpec(() => {
        try {
          return buildSpecFromPreset(preset, '')
        } catch {
          return preset.mentorPromptTemplate.replace('{task}', '(describe your task)')
        }
      })
    } else {
      setSpec((current) => {
        if (current && current.includes('ROLE: MENTOR')) {
          return stripTemplate(current)
        }
        return current
      })
    }
  }, [])

  const effectiveMentorModel = useMemo(
    () => pair?.pendingMentorModel ?? pair?.mentorModel ?? '',
    [pair]
  )
  const effectiveExecutorModel = useMemo(
    () => pair?.pendingExecutorModel ?? pair?.executorModel ?? '',
    [pair]
  )

  const modelsChanged =
    tempMentorModel !== effectiveMentorModel || tempExecutorModel !== effectiveExecutorModel

  if (!pair) return null

  const handleSkillSelect = (skillName: string) => {
    const insertion = `Load the ${skillName} skill and `
    setSpec((prev) => {
      if (textareaRef.current) {
        const start = textareaRef.current.selectionStart
        return prev.slice(0, start) + insertion + prev.slice(start)
      }
      return insertion + prev
    })
  }

  const handleSubmit = async (e: React.FormEvent): Promise<void> => {
    e.preventDefault()
    if (!spec.trim()) return

    const referencedFiles = Array.from(fileContexts.entries()).filter(([path]) =>
      spec.includes(`@${path}`)
    )
    let finalSpec = spec.trim()
    if (referencedFiles.length > 0) {
      const contextHeader =
        '--- REFERENCED FILES ---\n' +
        referencedFiles.map(([path, content]) => `@${path}:\n${content}`).join('\n\n') +
        '\n\n--- TASK ---\n'
      finalSpec = contextHeader + finalSpec
    }

    try {
      const modelOverrides = modelsChanged
        ? {
            mentorModel: tempMentorModel !== effectiveMentorModel ? tempMentorModel : undefined,
            executorModel:
              tempExecutorModel !== effectiveExecutorModel ? tempExecutorModel : undefined
          }
        : undefined
      await assignTask(pair.id, finalSpec, undefined, modelOverrides)
      setSpec('')
      setFileContexts(new Map())
      setRestoringSpec(null)
      setSelectedPreset(null)
      onClose()
    } catch {
      // Store already holds the user-facing error
    }
  }

  return (
    <GlassModal
      isOpen={isOpen}
      onClose={onClose}
      title={
        isRestoring
          ? t('modals.restoreTask', { name: pair.name })
          : t('modals.assignTask', { name: pair.name })
      }
      className="max-w-3xl"
    >
      <form
        onSubmit={handleSubmit}
        className="flex flex-col h-[70vh] max-h-[600px] font-mono text-[12px]"
      >
        <div className="flex-1 overflow-y-auto min-h-0 flex flex-col gap-3 scrollbar-thin">
          <div className="border border-border rounded-sm px-3 py-1.5">
            <div className="text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
              {t('modals.workspaceLabel')}
            </div>
            <div className="mt-0.5 truncate text-foreground/90" title={pair.directory}>
              {pair.directory}
            </div>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <ModelPicker
              value={tempMentorModel}
              models={availableModels}
              onChange={setTempMentorModel}
              role="mentor"
              variant="card"
            />
            <ModelPicker
              value={tempExecutorModel}
              models={availableModels}
              onChange={setTempExecutorModel}
              role="executor"
              variant="card"
            />
          </div>

          {modelsChanged && (
            <div className="border-l-2 border-role-mentor bg-role-mentor px-3 py-2 text-[11px] role-mentor">
              → {t('modals.modelUpdateNote')}
            </div>
          )}

          <div className="flex flex-col gap-1">
            <div className="flex items-baseline gap-1.5 text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
              <Sparkles size={10} className="state-running translate-y-px" />
              <span>{t('onboarding.workflowPreset')}</span>
            </div>
            <PresetPicker
              presets={presets}
              selectedPresetId={selectedPreset?.id ?? null}
              onSelect={handlePresetSelect}
              loading={presetsLoading}
              onRetry={loadPresets}
              error={presetsError}
            />
          </div>

          <div className="relative flex flex-col gap-1">
            <label className="text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
              {t('onboarding.taskSpec')}
            </label>
            <textarea
              ref={textareaRef}
              value={spec}
              onChange={(e) => setSpec(e.target.value)}
              placeholder="describe the next task for this pair. mention expected outcome, constraints, and how you want them to verify the work. use @filename to reference files."
              rows={6}
              className="w-full resize-none rounded-sm border border-border bg-background px-2 py-1.5 text-[12px] leading-relaxed text-foreground placeholder:text-muted-foreground-faint focus:outline-none focus:border-foreground/60"
              required
              data-testid="assign-task-spec"
            />
            <div className="absolute top-[26px] right-2 flex items-center gap-1">
              <SkillPicker projectDir={pair.directory} onSelect={handleSkillSelect} />
              <FileMention
                textareaRef={textareaRef}
                onChange={setSpec}
                directory={pair.directory}
                pairId={pair.id}
                onFileSelect={handleFileSelect}
              />
            </div>
            <p className="text-[10px] text-muted-foreground-faint">
              {spec.length > 0 ? `${spec.length} ${t('common.chars')} · ` : ''}· type @ to reference
              files · fresh planning loop on next run
            </p>
          </div>

          {error && (
            <div className="border-l-2 border-state-error bg-state-error/10 px-3 py-2 text-[11px] state-error">
              ✗ {error}
            </div>
          )}
        </div>

        <div className="flex justify-end gap-2 shrink-0 pt-3 border-t border-border mt-3">
          <GlassButton
            type="button"
            variant="ghost"
            onClick={() => {
              setRestoringSpec(null)
              setSelectedPreset(null)
              onClose()
            }}
            data-testid="assign-cancel-btn"
          >
            {t('common.cancel')}
          </GlassButton>
          <GlassButton
            type="submit"
            variant="primary"
            disabled={isLoading || spec.trim().length === 0}
            icon={isRestoring ? <RotateCcw size={11} /> : <ArrowUpRight size={11} />}
            data-testid="assign-submit-btn"
          >
            {isLoading
              ? t('modals.starting')
              : isRestoring
                ? t('modals.restore')
                : `▸ ${t('modals.startNewTask').toLowerCase()}`}
          </GlassButton>
        </div>
      </form>
    </GlassModal>
  )
}
