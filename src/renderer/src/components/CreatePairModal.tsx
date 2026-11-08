import React, { useState, useEffect, useRef, useCallback, useId } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { FolderOpen, RefreshCw } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { usePairStore } from '../store/usePairStore'
import { GlassModal } from './ui/GlassModal'
import { GlassButton } from './ui/GlassButton'
import { IconButton } from './ui/IconButton'
import { ModelPicker } from './ModelPicker'
import { BrandMark } from './BrandMark'
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
import {
  errorCallout,
  fieldInput,
  fieldLabel,
  hoverTint,
  sectionLabel,
  warningCallout
} from '../lib/ui'
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

/** Section rule: a terse uppercase heading and a hairline that fills the rest. */
function SectionLabel({ children }: { children: React.ReactNode }): React.ReactNode {
  return (
    <div className="flex items-center gap-3">
      <h3 className={cn(sectionLabel, 'shrink-0')}>{children}</h3>
      <span aria-hidden className="h-px flex-1 bg-border" />
    </div>
  )
}

/**
 * Field label. Required fields carry the brand accent asterisk; the native
 * `required` attribute on the control is what screen readers announce, and
 * the asterisk's tooltip spells the marker out for the mouse.
 */
function FieldLabel({
  htmlFor,
  children,
  required = false
}: {
  htmlFor: string
  children: React.ReactNode
  required?: boolean
}): React.ReactNode {
  const { t } = useTranslation()
  return (
    <label htmlFor={htmlFor} className={cn(fieldLabel, 'flex items-baseline gap-1')}>
      {children}
      {required && (
        <span
          aria-hidden
          title={t('modals.requiredHint')}
          className="font-mono text-[11px] leading-none text-state-running"
        >
          *
        </span>
      )}
    </label>
  )
}

/** A checkbox row: label plus a one-line explainer. */
function ToggleRow({
  checked,
  onToggle,
  label,
  hint,
  testId
}: {
  checked: boolean
  onToggle: () => void
  label: string
  hint: string
  testId: string
}): React.ReactNode {
  return (
    <button
      type="button"
      onClick={onToggle}
      aria-pressed={checked}
      data-testid={testId}
      className={cn(
        '-mx-1.5 flex cursor-pointer items-start gap-2 rounded-sm px-1.5 py-1 text-left transition-colors',
        hoverTint
      )}
    >
      <span
        aria-hidden
        className={cn(
          'mt-px select-none font-mono text-[12px]',
          checked ? 'state-running' : 'text-muted-foreground-faint'
        )}
      >
        {checked ? '[x]' : '[ ]'}
      </span>
      <span className="flex flex-col gap-0.5">
        <span className={fieldLabel}>{label}</span>
        <span className="text-[11px] normal-case leading-snug text-muted-foreground-faint">
          {hint}
        </span>
      </span>
    </button>
  )
}

/** What the active preset brings with it: skills it expects and its pause point. */
export function PresetHintBar({ preset }: { preset: PairPreset }): React.ReactNode {
  const { t } = useTranslation()
  return (
    <div className={warningCallout}>
      <div className="flex items-baseline gap-1.5 state-running uppercase tracking-[0.14em]">
        <span>→</span>
        <span>{t('modals.usingPreset', { name: preset.name })}</span>
      </div>
      <p className="mt-1 text-muted-foreground normal-case tracking-normal">
        {preset.recommendedSkills.length > 0 && (
          <>{t('pickers.presetSkills', { skills: preset.recommendedSkills.join(', ') })}</>
        )}
        {preset.pauseOnIteration && (
          <> · {t('pickers.presetAutoPause', { count: preset.pauseOnIteration })}</>
        )}
      </p>
    </div>
  )
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
  const [useWorktree, setUseWorktree] = useState(false)
  const [planGate, setPlanGate] = useState(false)
  const [recommendation, setRecommendation] = useState<ConfigRecommendation | null>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fieldId = useId()

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

  // A git repo with at least one branch defaults to an isolated worktree; the
  // user can always opt back into working in place. Non-git (or unborn, i.e.
  // no commits yet) folders keep the in-place default — there is nothing to
  // isolate from and no base to start the fresh branch from. Re-evaluated only
  // when the committed scan directory changes, so an explicit uncheck survives
  // until a new folder is picked.
  useEffect(() => {
    if (!scanDirectory) return
    let cancelled = false
    tauriApi.repo
      .checkState(scanDirectory)
      .then((state) => {
        if (!cancelled) setUseWorktree(state.isGitRepo && state.branches.length > 0)
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [scanDirectory])

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
        useWorktree,
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
      setUseWorktree(false)
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

  const workspaceReady = Boolean(scanDirectory) && scanDirectory === directory.trim()

  // Brand lockup: the logo mark next to the wordmark and tagline.
  const brandHeader = (
    <div className="flex min-w-0 items-center gap-3">
      <BrandMark variant="band" size={112} className="shrink-0" />
      <div className="flex min-w-0 flex-col gap-1">
        <h2 className="truncate text-[13px] font-semibold uppercase tracking-[0.16em] text-foreground">
          {t('modals.createTitle')}
        </h2>
        <p className="truncate text-[10px] leading-none tracking-[0.06em] text-muted-foreground-faint">
          <span className="uppercase tracking-[0.22em]">{t('startup.wordmark')}</span>
          <span aria-hidden> · </span>
          <span>{t('startup.tagline')}</span>
        </p>
      </div>
    </div>
  )

  return (
    <GlassModal
      isOpen={isOpen}
      onClose={onClose}
      title={t('modals.createTitle')}
      header={brandHeader}
      className="max-w-2xl"
    >
      <form onSubmit={handleSubmit} className="flex flex-col text-[13px]">
        {/* -mx/px gives the toggles' -mx-1.5 hover tint room inside the scroll clip. */}
        <div className="-mx-1.5 flex flex-1 flex-col gap-5 overflow-y-auto px-1.5">
          {/* ── Task ─────────────────────────────────────────────────────── */}
          <div className="flex flex-col gap-2.5">
            <SectionLabel>{t('common.task')}</SectionLabel>

            <div className="flex flex-col gap-1.5">
              <span className={fieldLabel}>{t('modals.choosePreset')}</span>
              <PresetPicker
                presets={presets}
                selectedPresetId={selectedPreset?.id ?? null}
                onSelect={handlePresetSelect}
                loading={presetsLoading}
                onRetry={loadPresets}
                error={presetsError}
              />
            </div>

            {selectedPreset && <PresetHintBar preset={selectedPreset} />}

            <div className="flex flex-col gap-1.5">
              <FieldLabel htmlFor={`${fieldId}-spec`} required>
                {t('onboarding.taskSpec')}
              </FieldLabel>
              <textarea
                id={`${fieldId}-spec`}
                ref={textareaRef}
                value={spec}
                onChange={(e) => setSpec(e.target.value)}
                placeholder={t('onboarding.taskPlaceholder')}
                rows={4}
                className={cn(fieldInput, 'resize-none leading-relaxed')}
                required
                data-testid="pair-task-spec"
              />
              <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
                <p className="text-[10px] leading-snug text-muted-foreground-faint">
                  {t('modals.mentionHint')}
                </p>
                {/* Right-aligned: the skill dropdown opens leftwards from its
                    trigger, so the trigger has to sit near the modal's edge. */}
                {workspaceReady && (
                  <div className="flex items-center gap-1">
                    <SkillPicker projectDir={scanDirectory} onSelect={handleSkillSelect} />
                    <FileMention
                      textareaRef={textareaRef}
                      onChange={setSpec}
                      directory={scanDirectory}
                      onFileSelect={handleFileSelect}
                    />
                  </div>
                )}
              </div>
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
              <div className={warningCallout}>! {t('modals.presetTaskMissing')}</div>
            )}
          </div>

          {/* ── Workspace ────────────────────────────────────────────────── */}
          <div className="flex flex-col gap-2.5">
            <SectionLabel>{t('common.workspace')}</SectionLabel>

            <div className="flex flex-col gap-1.5">
              <FieldLabel htmlFor={`${fieldId}-name`} required>
                {t('common.name')}
              </FieldLabel>
              <input
                id={`${fieldId}-name`}
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={preventImeEnterSubmit}
                placeholder={t('onboarding.pairNamePlaceholder')}
                className={fieldInput}
                required
                data-testid="pair-name-input"
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <FieldLabel htmlFor={`${fieldId}-directory`} required>
                {t('common.directory')}
              </FieldLabel>
              <div className="flex gap-1.5">
                <input
                  id={`${fieldId}-directory`}
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
                  placeholder={t('modals.directoryPlaceholder')}
                  className={cn(fieldInput, 'min-w-0 flex-1', !directory && 'cursor-pointer')}
                  required
                  data-testid="pair-directory-input"
                />
                {/* h-auto lets the row stretch it to the input's height: a square, aligned pair. */}
                <IconButton
                  size="md"
                  bordered
                  onClick={handleSelectDirectory}
                  aria-label={t('modals.pickDirectory')}
                  className="h-auto w-8 rounded-sm"
                >
                  <FolderOpen size={13} />
                </IconButton>
              </div>
            </div>

            {workspaceReady && (
              <div className="flex flex-col gap-1.5">
                <ToggleRow
                  checked={useWorktree}
                  onToggle={() => {
                    setUseWorktree((v) => !v)
                    // The selection means different things in the two modes
                    // (work on the branch vs. start from it), so start clean.
                    setBranch(undefined)
                  }}
                  label={t('modals.worktreeLabel')}
                  hint={t('modals.worktreeHint')}
                  testId="worktree-toggle"
                />
                {useWorktree ? (
                  <>
                    <BranchPicker
                      directory={scanDirectory}
                      value={branch}
                      onChange={setBranch}
                      variant="base"
                    />
                    <div className={warningCallout}>→ {t('modals.worktreePreview')}</div>
                  </>
                ) : (
                  <BranchPicker directory={scanDirectory} value={branch} onChange={setBranch} />
                )}
              </div>
            )}
          </div>

          {/* ── Roles ────────────────────────────────────────────────────── */}
          <div className="flex flex-col gap-2.5">
            <SectionLabel>{t('modals.sectionRoles')}</SectionLabel>

            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
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

            <ToggleRow
              checked={planGate}
              onToggle={() => setPlanGate((v) => !v)}
              label={t('modals.planGateLabel')}
              hint={t('modals.planGateHint')}
              testId="plan-gate-toggle"
            />

            {modelsError && (
              <div role="alert" className={cn(errorCallout, 'flex flex-col gap-1.5')}>
                <span>✗ {t('modals.modelsError')}</span>
                <span className="text-[11px] normal-case tracking-normal text-muted-foreground">
                  {t('modals.modelsErrorHint')}
                </span>
                <span className="text-[10px] normal-case tracking-normal text-muted-foreground-faint">
                  {modelsError}
                </span>
                <GlassButton
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => void loadAvailableModels()}
                  icon={<RefreshCw size={12} />}
                  className="self-start"
                >
                  {t('common.retry')}
                </GlassButton>
              </div>
            )}
          </div>

          {error && (
            <div role="alert" className={cn(errorCallout, 'flex flex-col gap-1')}>
              <span>✗ {error}</span>
              <span className="text-[11px] normal-case tracking-normal text-muted-foreground">
                {t('modals.createErrorHint')}
              </span>
            </div>
          )}
        </div>

        {/* The footer rides the bottom of the scroll area: the dialog is
            content-sized, so on a short window the form is taller than the
            card and the submit button has to stay reachable. */}
        <div className="sticky bottom-0 -mx-1.5 mt-1 flex shrink-0 justify-end gap-2 border-t border-border bg-surface-raised px-1.5 pt-3">
          <GlassButton
            type="button"
            variant="secondary"
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
            {isLoading ? t('modals.creating') : t('modals.create')}
          </GlassButton>
        </div>
      </form>
    </GlassModal>
  )
}
