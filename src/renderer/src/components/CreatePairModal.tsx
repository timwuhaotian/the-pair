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
import { buildSpecFromPreset, stripTemplate } from '../lib/presetUtils'
import { usePresets } from '../lib/usePresets'
import type { PairPreset } from '../types'

interface CreatePairModalProps {
  isOpen: boolean
  onClose: () => void
}

export function CreatePairModal({ isOpen, onClose }: CreatePairModalProps): React.ReactNode {
  const { t } = useTranslation()
  const { availableModels, loadAvailableModels, createPair, isLoading, error } = usePairStore()

  const [name, setName] = useState('')
  const [directory, setDirectory] = useState('')
  const [spec, setSpec] = useState('')
  const [mentorModel, setMentorModel] = useState('')
  const [executorModel, setExecutorModel] = useState('')
  const [mentorReasoningEffort, setMentorReasoningEffort] = useState<string | undefined>()
  const [executorReasoningEffort, setExecutorReasoningEffort] = useState<string | undefined>()
  const [fileContexts, setFileContexts] = useState<Map<string, string>>(new Map())
  const [branch, setBranch] = useState<string | undefined>()
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const [selectedPreset, setSelectedPreset] = useState<PairPreset | null>(null)
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

  const handlePresetSelect = useCallback((preset: PairPreset | null) => {
    setSelectedPreset(preset)
    if (preset) {
      if (preset.recommendedMentorModel) {
        setMentorModel(preset.recommendedMentorModel)
      }
      if (preset.recommendedExecutorModel) {
        setExecutorModel(preset.recommendedExecutorModel)
      }
      if (preset.mentorPromptTemplate) {
        setSpec(() => {
          try {
            return buildSpecFromPreset(preset, '')
          } catch {
            return preset.mentorPromptTemplate.replace('{task}', '(describe your task)')
          }
        })
      }
    } else {
      setSpec((current) => {
        if (current && current.includes('ROLE: MENTOR')) {
          return stripTemplate(current)
        }
        return current
      })
    }
  }, [])

  const handleSubmit = async (e: React.FormEvent): Promise<void> => {
    e.preventDefault()
    try {
      const referencedFiles = Array.from(fileContexts.entries()).filter(([path]) =>
        spec.includes(`@${path}`)
      )
      let finalSpec = spec
      if (selectedPreset && !finalSpec.includes('ROLE: MENTOR')) {
        try {
          finalSpec = buildSpecFromPreset(selectedPreset, finalSpec)
        } catch {
          finalSpec = selectedPreset.mentorPromptTemplate.replace(
            '{task}',
            finalSpec || '(describe your task)'
          )
        }
      }
      if (referencedFiles.length > 0) {
        const contextHeader =
          '--- REFERENCED FILES ---\n' +
          referencedFiles.map(([path, content]) => `@${path}:\n${content}`).join('\n\n') +
          '\n\n--- TASK ---\n'
        finalSpec = contextHeader + finalSpec
      }
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
        pauseOnIteration: selectedPreset?.pauseOnIteration,
        autoAttachGitBaseline: selectedPreset?.autoAttachGitBaseline
      })
      setName('')
      setDirectory('')
      setSpec('')
      setFileContexts(new Map())
      setMentorReasoningEffort(undefined)
      setExecutorReasoningEffort(undefined)
      setBranch(undefined)
      setSelectedPreset(null)
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

  const handleSelectDirectory = async (): Promise<void> => {
    const selected = await open({
      directory: true,
      multiple: false
    })
    if (selected) {
      setDirectory(selected)
      setName((currentName) =>
        currentName.trim().length > 0 ? currentName : derivePairNameFromDirectory(selected)
      )
    }
  }

  return (
    <GlassModal
      isOpen={isOpen}
      onClose={onClose}
      title={t('modals.createTitle')}
      className="max-w-3xl"
    >
      <form onSubmit={handleSubmit} className="flex flex-col h-full">
        <div className="flex flex-col gap-3 overflow-y-auto flex-1 pr-1 -mr-1">
          <div>
            <div className="mb-1.5 flex items-center gap-2">
              <Sparkles size={14} className="text-primary" />
              <span className="text-sm font-medium text-foreground">
                {t('modals.choosePreset')}
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

          {selectedPreset && (
            <div className="rounded-xl border border-primary/20 bg-primary/5 px-3.5 py-2.5">
              <div className="flex items-center gap-2">
                <Sparkles size={12} className="text-primary" />
                <span className="text-xs font-medium text-primary">
                  {t('modals.usingPreset', { name: selectedPreset.name })}
                </span>
              </div>
              <p className="mt-1 text-xs text-muted-foreground">
                {selectedPreset.recommendedSkills.length > 0 && (
                  <>Skills: {selectedPreset.recommendedSkills.join(', ')}</>
                )}
                {selectedPreset.pauseOnIteration && (
                  <> • Auto-pause at iteration {selectedPreset.pauseOnIteration}</>
                )}
              </p>
            </div>
          )}

          <div>
            <label className="block text-sm font-medium text-foreground mb-1.5">
              {t('common.name')}
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('onboarding.pairNamePlaceholder')}
              className="w-full px-3.5 py-2 glass-card text-sm text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-2 focus:ring-primary/20 rounded-xl"
              required
              data-testid="pair-name-input"
            />
          </div>

          <div>
            <label className="block text-sm font-medium text-foreground mb-1.5">
              {t('common.directory')}
            </label>
            <div className="flex gap-2">
              <input
                type="text"
                value={directory}
                onChange={(e) => setDirectory(e.target.value)}
                placeholder="/path/to/project"
                className="flex-1 px-3.5 py-2 glass-card text-sm font-mono text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-2 focus:ring-primary/20 rounded-xl"
                required
                data-testid="pair-directory-input"
              />
              <GlassButton
                type="button"
                variant="secondary"
                size="md"
                onClick={handleSelectDirectory}
                icon={<FolderOpen size={15} />}
              >
                {''}
              </GlassButton>
            </div>
          </div>

          {directory && <BranchPicker directory={directory} value={branch} onChange={setBranch} />}

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

          <div className="relative">
            <label className="block text-sm font-medium text-foreground mb-1.5">
              {t('onboarding.taskSpec')}
            </label>
            <textarea
              ref={textareaRef}
              value={spec}
              onChange={(e) => setSpec(e.target.value)}
              placeholder={t('onboarding.taskPlaceholder')}
              rows={4}
              className="w-full px-3.5 py-2 glass-card text-sm text-foreground placeholder:text-muted-foreground/50 resize-none focus:outline-none focus:ring-2 focus:ring-primary/20 rounded-xl leading-relaxed"
              required
              data-testid="pair-task-spec"
            />
            <div className="absolute top-8 right-2 flex items-center gap-1">
              {directory && (
                <>
                  <SkillPicker projectDir={directory} onSelect={handleSkillSelect} />
                  <FileMention
                    textareaRef={textareaRef}
                    onChange={setSpec}
                    directory={directory}
                    onFileSelect={handleFileSelect}
                  />
                </>
              )}
            </div>
            <p className="mt-1 text-xs text-muted-foreground">
              Type @ to reference workspace files
            </p>
          </div>

          {error && (
            <div className="text-sm text-destructive glass-card p-3 rounded-xl border border-destructive/20">
              {error}
            </div>
          )}
        </div>

        <div className="flex justify-end gap-3 pt-3 border-t border-border/40 mt-3 flex-shrink-0">
          <GlassButton
            type="button"
            variant="ghost"
            onClick={onClose}
            data-testid="pair-cancel-btn"
          >
            Cancel
          </GlassButton>
          <GlassButton
            type="submit"
            variant="primary"
            disabled={isLoading}
            data-testid="pair-submit-btn"
          >
            {isLoading ? t('modals.creating') : t('modals.create')}
          </GlassButton>
        </div>
      </form>
    </GlassModal>
  )
}
