import React, { useMemo, useState, useRef, useCallback } from 'react'
import { ArrowUpRight, Sparkles, RotateCcw } from 'lucide-react'
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
      title={isRestoring ? `Restore Task · ${pair.name}` : `Assign New Task · ${pair.name}`}
      className="max-w-3xl"
    >
      <form onSubmit={handleSubmit} className="flex flex-col h-[70vh] max-h-[600px]">
        <div className="flex-1 overflow-y-auto min-h-0 flex flex-col gap-4">
          <div className="glass-card rounded-xl p-3">
            <div className="text-[10px] font-semibold uppercase tracking-[0.2em] text-muted-foreground">
              Workspace
            </div>
            <div className="mt-1 truncate font-mono text-xs text-foreground" title={pair.directory}>
              {pair.directory}
            </div>
          </div>
          <div className="grid grid-cols-2 gap-4">
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
            <div className="rounded-xl border border-blue-500/20 bg-blue-500/10 px-3.5 py-2.5 text-sm text-blue-700 dark:text-blue-300">
              Updated models will become the new defaults for this pair.
            </div>
          )}

          <div>
            <div className="mb-2 flex items-center gap-1.5">
              <Sparkles size={11} className="text-primary" />
              <span className="text-[10px] font-semibold uppercase tracking-[0.15em] text-muted-foreground">
                Workflow Preset
              </span>
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

          <div className="relative">
            <label className="mb-2 block text-sm font-medium text-foreground">
              Task Specification
            </label>
            <textarea
              ref={textareaRef}
              value={spec}
              onChange={(e) => setSpec(e.target.value)}
              placeholder="Describe the next task for this pair. Mention expected outcome, constraints, and how you want them to verify the work. Use @filename to reference files."
              rows={6}
              className="glass-card w-full resize-none rounded-xl px-3.5 py-2.5 text-sm leading-relaxed text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-2 focus:ring-primary/20"
              required
              data-testid="assign-task-spec"
            />
            <div className="absolute top-8 right-2 flex items-center gap-1">
              <SkillPicker projectDir={pair.directory} onSelect={handleSkillSelect} />
              <FileMention
                textareaRef={textareaRef}
                onChange={setSpec}
                directory={pair.directory}
                pairId={pair.id}
                onFileSelect={handleFileSelect}
              />
            </div>
            <p className="mt-1 text-xs text-muted-foreground/70">
              {spec.length > 0 ? `${spec.length} characters · ` : ''}Type @ to reference files. The
              next run starts with a fresh planning loop.
            </p>
          </div>

          {error && (
            <div className="rounded-xl border border-destructive/20 bg-destructive/10 px-3.5 py-2.5 text-sm text-destructive">
              {error}
            </div>
          )}
        </div>

        <div className="flex justify-end gap-3 shrink-0 pt-3 border-t border-border/40 mt-4">
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
            Cancel
          </GlassButton>
          <GlassButton
            type="submit"
            variant="primary"
            disabled={isLoading || spec.trim().length === 0}
            icon={isRestoring ? <RotateCcw size={14} /> : <ArrowUpRight size={14} />}
            data-testid="assign-submit-btn"
          >
            {isLoading ? 'Starting...' : isRestoring ? 'Restore Task' : 'Start New Task'}
          </GlassButton>
        </div>
      </form>
    </GlassModal>
  )
}
