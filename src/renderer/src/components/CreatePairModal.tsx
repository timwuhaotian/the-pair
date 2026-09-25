import React, { useState, useEffect, useRef, useCallback } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { FolderOpen, Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { usePairStore } from '../store/usePairStore'
import { GlassModal } from './ui/GlassModal'
import { GlassButton } from './ui/GlassButton'
import { ModelPicker } from './ModelPicker'
import { getPreferredQualifiedModel } from '../lib/modelPreferences'
import { FileMention } from './FileMention'
import { SkillPicker } from './SkillPicker'
import { derivePairNameFromDirectory } from '../lib/workspace'
import { BranchPicker } from './BranchPicker'
import { PresetPicker } from './PresetPicker'
import { usePresets } from '../lib/usePresets'
import { prependFileContext } from '../lib/fileMentions'
import { isTauri, tauriApi } from '../lib/tauri-api'
import { cn } from '../lib/utils'
import type { ConfigRecommendation, PairPreset } from '../types'
import { preventImeEnterSubmit } from './keyboard'
import {
  finalizePresetSpec,
  isPresetTaskMissing,
  removePresetTemplate,
  switchPresetTemplate
} from './presetSpec'

interface CreatePairModalProps {
  isOpen: boolean
  onClose: () => void
}

export function CreatePairModal({ isOpen, onClose }: CreatePairModalProps): React.ReactNode {
  const { t } = useTranslation()
  const availableModels = usePairStore((s) => s.availableModels)
  const loadAvailableModels = usePairStore((s) => s.loadAvailableModels)
  const createPair = usePairStore((s) => s.createPair)
  const isLoading = usePairStore((s) => s.isLoading)
  const error = usePairStore((s) => s.error)
  const modelsError = usePairStore((s) => s.modelsError)

  const [name, setName] = useState('')
  const [directory, setDirectory] = useState('')
  // The directory repo/file scans run against. Only committed via the folder
  // picker or when the path input loses focus — never on every keystroke, where
  // each existing prefix (e.g. the home dir) would trigger a full recursive scan.
  const [scanDirectory, setScanDirectory] = useState('')
  const [spec, setSpec] = useState('')
  const [mentorModel, setMentorModel] = useState('')
  const [executorModel, setExecutorModel] = useState('')
  const [mentorReasoningEffort, setMentorReasoningEffort] = useState<string | undefined>()
  const [executorReasoningEffort, setExecutorReasoningEffort] = useState<string | undefined>()
  const [fileContexts, setFileContexts] = useState<Map<string, string>>(new Map())
  const [branch, setBranch] = useState<string | undefined>()
  const [planGate, setPlanGate] = useState(false)
  const [recommendation, setRecommendation] = useState<ConfigRecommendation | null>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const [selectedPreset, setSelectedPreset] = useState<PairPreset | null>(null)
  // The preset whose template currently wraps the textarea text (if any).
  const [appliedPreset, setAppliedPreset] = useState<PairPreset | null>(null)
  const {
    presets,
    loading: presetsLoading,
    error: presetsError,
    reload: loadPresets
  } = usePresets()

  const handleFileSelect = useCallback((path: string, content: string): void => {
    setFileContexts((prev) => {
      const next = new Map(prev)
      next.set(path, content)
      return next
    })
  }, [])

  // Debounced cross-run recommendation for the draft task spec. Stays silent
  // when there's no matching history or the backend has nothing to recommend.
  useEffect(() => {
    const handle = setTimeout(() => {
      const text = spec.trim()
      if (!isTauri || text.length < 8) {
        setRecommendation(null)
        return
      }
      tauriApi.insights
        .getRecommendation(text)
        .then((recommendations) => setRecommendation(recommendations[0] ?? null))
        .catch(() => setRecommendation(null))
    }, 500)
    return () => clearTimeout(handle)
  }, [spec])

  // A stale global error (e.g. from a pair's handoff) must not greet the user
  // in a freshly opened modal.
  useEffect(() => {
    if (isOpen) usePairStore.setState({ error: null })
  }, [isOpen])

  useEffect(() => {
    if (isOpen && availableModels.length === 0) {
      loadAvailableModels()
    }
  }, [isOpen, availableModels.length, loadAvailableModels])

  useEffect(() => {
    if (isOpen) {
      void loadPresets()
    }
  }, [isOpen, loadPresets])

  useEffect(() => {
    if (availableModels.length > 0 && mentorModel === '') {
      const mentorDefault = getPreferredQualifiedModel('mentor', availableModels)
      const executorDefault = getPreferredQualifiedModel('executor', availableModels)
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setMentorModel(mentorDefault)

      setExecutorModel(executorDefault)
    }
  }, [availableModels, mentorModel])

  const handlePresetSelect = useCallback(
    (preset: PairPreset | null) => {
      setSelectedPreset(preset)
      if (preset) {
        if (preset.recommendedMentorModel) {
          setMentorModel(preset.recommendedMentorModel)
        }
        if (preset.recommendedExecutorModel) {
          setExecutorModel(preset.recommendedExecutorModel)
        }
        if (preset.mentorPromptTemplate) {
          // Carry the typed task into the new template instead of wrapping twice.
          setSpec((current) => switchPresetTemplate(appliedPreset, preset, current))
          setAppliedPreset(preset)
        }
      } else {
        const previous = appliedPreset
        if (previous) setSpec((current) => removePresetTemplate(previous, current))
        setAppliedPreset(null)
      }
    },
    [appliedPreset]
  )

  const presetTaskMissing = isPresetTaskMissing(appliedPreset, spec)

  const handleSubmit = async (e: React.FormEvent): Promise<void> => {
    e.preventDefault()
    if (presetTaskMissing) return
    try {
      // Wraps only if the selected preset's template isn't already in the textarea.
      let finalSpec = finalizePresetSpec(selectedPreset, appliedPreset, spec)
      finalSpec = prependFileContext(finalSpec, fileContexts)
      await createPair({
        name,
        directory,
        spec: finalSpec,
        mentorModel,
        executorModel,
        mentorReasoningEffort,
        executorReasoningEffort,
        branch,
        maxIterations: undefined,
        planGate,
        pauseOnIteration: selectedPreset?.pauseOnIteration,
        autoAttachGitBaseline: selectedPreset?.autoAttachGitBaseline
      })
      setName('')
      setDirectory('')
      setScanDirectory('')
      setSpec('')
      setFileContexts(new Map())
      setMentorReasoningEffort(undefined)
      setExecutorReasoningEffort(undefined)
      setBranch(undefined)
      setPlanGate(false)
      setSelectedPreset(null)
      setAppliedPreset(null)
      onClose()
    } catch {
      // Store already exposes the error copy
    }
  }

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

  // A branch belongs to the repo it was picked from — drop it whenever the
  // directory changes so createPair never receives a stale, hidden branch.
  const changeDirectory = (next: string): void => {
    if (next !== directory) setBranch(undefined)
    setDirectory(next)
  }

  const commitScanDirectory = (next: string): void => {
    const trimmed = next.trim()
    if (trimmed !== scanDirectory) {
      setBranch(undefined)
      setScanDirectory(trimmed)
    }
  }

  const handleSelectDirectory = async (): Promise<void> => {
    try {
      const selected = await open({
        directory: true,
        multiple: false
      })
      if (selected) {
        changeDirectory(selected)
        commitScanDirectory(selected)
        setName((currentName) =>
          currentName.trim().length > 0 ? currentName : derivePairNameFromDirectory(selected)
        )
      }
    } catch (err) {
      console.error('[CreatePairModal] Folder picker failed:', err)
    }
  }

  return (
    <GlassModal
      isOpen={isOpen}
      onClose={onClose}
      title={t('modals.createTitle')}
      className="max-w-3xl"
    >
      <form onSubmit={handleSubmit} className="flex flex-col h-full font-mono text-[12px]">
        <div className="flex flex-col gap-3 overflow-y-auto flex-1 pr-1 -mr-1">
          <div>
            <div className="mb-1 flex items-baseline gap-1.5 text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
              <Sparkles size={11} className="state-running translate-y-px" />
              <span>{t('modals.choosePreset')}</span>
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

          {selectedPreset && (
            <div className="border-l-2 border-state-running/40 bg-state-running/8 px-3 py-2 text-[11px]">
              <div className="flex items-baseline gap-1.5 state-running uppercase tracking-[0.14em]">
                <span>→</span>
                <span>{t('modals.usingPreset', { name: selectedPreset.name })}</span>
              </div>
              <p className="mt-1 text-muted-foreground normal-case tracking-normal">
                {selectedPreset.recommendedSkills.length > 0 && (
                  <>skills: {selectedPreset.recommendedSkills.join(', ')}</>
                )}
                {selectedPreset.pauseOnIteration && (
                  <> · auto-pause @ iter {selectedPreset.pauseOnIteration}</>
                )}
              </p>
            </div>
          )}

          <div className="flex flex-col gap-1">
            <label className="text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
              {t('common.name')}
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={preventImeEnterSubmit}
              placeholder={t('onboarding.pairNamePlaceholder')}
              className="w-full px-2 py-1.5 bg-background border border-border text-[12px] text-foreground placeholder:text-muted-foreground-faint focus:outline-none focus:border-foreground/60 rounded-sm"
              required
              data-testid="pair-name-input"
            />
          </div>

          <div className="flex flex-col gap-1">
            <label className="text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
              {t('common.directory')}
            </label>
            <div className="flex gap-1.5">
              <input
                type="text"
                value={directory}
                onChange={(e) => changeDirectory(e.target.value)}
                onBlur={(e) => commitScanDirectory(e.target.value)}
                onKeyDown={preventImeEnterSubmit}
                onClick={() => {
                  if (!directory) {
                    void handleSelectDirectory()
                  }
                }}
                placeholder="/path/to/project"
                className={cn(
                  'flex-1 px-2 py-1.5 bg-background border border-border text-[12px] text-foreground placeholder:text-muted-foreground-faint focus:outline-none focus:border-foreground/60 rounded-sm',
                  !directory && 'cursor-pointer'
                )}
                required
                data-testid="pair-directory-input"
              />
              <GlassButton
                type="button"
                variant="secondary"
                size="sm"
                onClick={handleSelectDirectory}
                icon={<FolderOpen size={13} />}
              >
                {''}
              </GlassButton>
            </div>
          </div>

          {scanDirectory && scanDirectory === directory.trim() && (
            <BranchPicker directory={scanDirectory} value={branch} onChange={setBranch} />
          )}

          <button
            type="button"
            onClick={() => setPlanGate((v) => !v)}
            aria-pressed={planGate}
            data-testid="plan-gate-toggle"
            className="flex items-start gap-2 text-left"
          >
            <span
              className={cn(
                'mt-px select-none font-mono text-[12px]',
                planGate ? 'state-running' : 'text-muted-foreground-faint'
              )}
            >
              {planGate ? '[x]' : '[ ]'}
            </span>
            <span className="flex flex-col gap-0.5">
              <span className="text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
                {t('modals.planGateLabel')}
              </span>
              <span className="text-[11px] normal-case leading-snug text-muted-foreground-faint">
                {t('modals.planGateHint')}
              </span>
            </span>
          </button>

          <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
            <ModelPicker
              value={mentorModel}
              models={availableModels}
              onChange={setMentorModel}
              role="mentor"
              variant="card"
              reasoningEffort={mentorReasoningEffort}
              onReasoningEffortChange={setMentorReasoningEffort}
            />
            <ModelPicker
              value={executorModel}
              models={availableModels}
              onChange={setExecutorModel}
              role="executor"
              variant="card"
              reasoningEffort={executorReasoningEffort}
              onReasoningEffortChange={setExecutorReasoningEffort}
            />
          </div>
          {modelsError && (
            <div
              role="alert"
              className="border-l-2 border-state-error bg-state-error/10 px-3 py-2 text-[11px] state-error"
            >
              ✗ {modelsError}
            </div>
          )}

          <div className="relative flex flex-col gap-1">
            <label className="text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
              {t('onboarding.taskSpec')}
            </label>
            <textarea
              ref={textareaRef}
              value={spec}
              onChange={(e) => setSpec(e.target.value)}
              placeholder={t('onboarding.taskPlaceholder')}
              rows={4}
              className="w-full px-2 py-1.5 bg-background border border-border text-[12px] text-foreground placeholder:text-muted-foreground-faint resize-none focus:outline-none focus:border-foreground/60 rounded-sm leading-relaxed"
              required
              data-testid="pair-task-spec"
            />
            <div className="absolute top-[26px] right-2 flex items-center gap-1">
              {scanDirectory && scanDirectory === directory.trim() && (
                <>
                  <SkillPicker projectDir={scanDirectory} onSelect={handleSkillSelect} />
                  <FileMention
                    textareaRef={textareaRef}
                    onChange={setSpec}
                    directory={scanDirectory}
                    onFileSelect={handleFileSelect}
                  />
                </>
              )}
            </div>
            <p className="text-[10px] text-muted-foreground-faint">
              · type @ to reference workspace files
            </p>
            {recommendation && (
              <p
                data-testid="recommendation-hint"
                className="text-[10px] leading-snug state-running"
              >
                ▸{' '}
                {t('modals.recommendationHint', {
                  mentor: recommendation.mentorModel,
                  executor: recommendation.executorModel,
                  successes: recommendation.successes,
                  runs: recommendation.runs
                })}
              </p>
            )}
          </div>

          {presetTaskMissing && (
            <div className="border-l-2 border-state-running bg-state-running/10 px-3 py-2 text-[11px] state-running">
              ! {t('modals.presetTaskMissing')}
            </div>
          )}

          {error && (
            <div className="border-l-2 border-state-error bg-state-error/10 px-3 py-2 text-[11px] state-error">
              ✗ {error}
            </div>
          )}
        </div>

        <div className="flex justify-end gap-2 pt-3 border-t border-border mt-3 flex-shrink-0">
          <GlassButton
            type="button"
            variant="ghost"
            onClick={onClose}
            data-testid="pair-cancel-btn"
          >
            {t('common.cancel')}
          </GlassButton>
          <GlassButton
            type="submit"
            variant="primary"
            disabled={isLoading || presetTaskMissing}
            data-testid="pair-submit-btn"
          >
            {isLoading ? t('modals.creating') : `▸ ${t('modals.create').toLowerCase()}`}
          </GlassButton>
        </div>
      </form>
    </GlassModal>
  )
}
