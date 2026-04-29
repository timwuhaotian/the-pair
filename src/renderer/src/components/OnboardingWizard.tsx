import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import {
  CheckCircle2,
  FolderOpen,
  ExternalLink,
  Rocket,
  Sun,
  Moon,
  AlertCircle,
  RefreshCw,
  Sparkles
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { usePairStore } from '../store/usePairStore'
import { useThemeStore } from '../store/useThemeStore'
import type { AvailableModel, PairPreset } from '../types'
import { GlassButton } from './ui/GlassButton'
import { GlassCard } from './ui/GlassCard'
import { ModelPicker } from './ModelPicker'
import { FileMention } from './FileMention'
import { SkillPicker } from './SkillPicker'
import { BranchPicker } from './BranchPicker'
import { PresetPicker } from './PresetPicker'
import { getPreferredPairModelSelection } from '../lib/modelPreferences'
import { derivePairNameFromDirectory } from '../lib/workspace'
import { shouldUseCompactOnboardingLayout } from '../lib/onboardingLayout'
import { buildProviderSetupSummary } from '../lib/providerSetup'
import { buildSpecFromPreset, stripTemplate } from '../lib/presetUtils'
import { usePresets } from '../lib/usePresets'
import type { ProviderSetupSummary } from '../lib/providerSetup'
import appIcon from '../assets/app-icon.png'

interface OnboardingWizardProps {
  onComplete: () => void
}

export function OnboardingWizard({ onComplete }: OnboardingWizardProps): React.ReactNode {
  const [appVersion, setAppVersion] = useState<string>('1.0.1')
  const [isCheckingProviders, setIsCheckingProviders] = useState(true)
  const [directory, setDirectory] = useState('')
  const [name, setName] = useState('')
  const [spec, setSpec] = useState('')
  const [mentorModel, setMentorModel] = useState('')
  const [executorModel, setExecutorModel] = useState('')
  const [isOpeningFile, setIsOpeningFile] = useState(false)
  const [isCreating, setIsCreating] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [fileContexts, setFileContexts] = useState<Map<string, string>>(new Map())
  const [branch, setBranch] = useState<string | undefined>()
  const [selectedPreset, setSelectedPreset] = useState<PairPreset | null>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const [viewportHeight, setViewportHeight] = useState(() =>
    typeof window === 'undefined' ? 900 : window.innerHeight
  )
  const isCompactLayout = shouldUseCompactOnboardingLayout(viewportHeight)
  const {
    presets,
    loading: presetsLoading,
    error: presetsError,
    reload: loadPresets
  } = usePresets()

  useEffect(() => {
    window.api?.config?.getVersion?.().then((v: string) => {
      setAppVersion(v && v !== '0.0.0' ? v : '1.0.1')
    })
  }, [])

  useEffect(() => {
    const updateViewportHeight = (): void => {
      setViewportHeight(window.innerHeight)
    }

    updateViewportHeight()
    window.addEventListener('resize', updateViewportHeight)
    return () => window.removeEventListener('resize', updateViewportHeight)
  }, [])

  const handleFileSelect = useCallback((path: string, content: string): void => {
    setFileContexts((prev) => {
      const next = new Map(prev)
      next.set(path, content)
      return next
    })
  }, [])

  const handleSkillSelect = useCallback((skillName: string) => {
    const insertion = `Load the ${skillName} skill and `
    setSpec((prev) => {
      if (textareaRef.current) {
        const start = textareaRef.current.selectionStart
        return prev.slice(0, start) + insertion + prev.slice(start)
      }
      return insertion + prev
    })
  }, [])

  const handlePresetSelect = useCallback((preset: PairPreset | null) => {
    setSelectedPreset(preset)
    if (preset) {
      if (preset.recommendedMentorModel) {
        setMentorModel(preset.recommendedMentorModel)
      }
      if (preset.recommendedExecutorModel) {
        setExecutorModel(preset.recommendedExecutorModel)
      }
      setSpec(() => {
        try {
          return buildSpecFromPreset(preset, '')
        } catch {
          return preset.mentorPromptTemplate.replace('{task}', '(describe your task)')
        }
      })
      setName((prev) => (prev.trim() ? prev : preset.name))
    } else {
      setSpec((current) => {
        if (current && current.includes('ROLE: MENTOR')) {
          return stripTemplate(current)
        }
        return current
      })
    }
  }, [])

  const { availableModels, loadAvailableModels, createPair } = usePairStore()
  const { theme, toggleTheme } = useThemeStore()
  const providerSummary = useMemo(
    () => buildProviderSetupSummary(availableModels),
    [availableModels]
  )

  useEffect(() => {
    if (availableModels.length > 0) {
      setIsCheckingProviders(false)
      return
    }
    let cancelled = false
    setIsCheckingProviders(true)
    void (async () => {
      await loadAvailableModels()
      if (!cancelled) {
        setIsCheckingProviders(false)
      }
    })()

    return () => {
      cancelled = true
    }
  }, [loadAvailableModels, availableModels.length])

  useEffect(() => {
    if (availableModels.length > 0 && mentorModel === '' && executorModel === '') {
      const defaults = getPreferredPairModelSelection(availableModels)
      setMentorModel(defaults.mentorModel)
      setExecutorModel(defaults.executorModel)
    }
  }, [availableModels, executorModel, mentorModel])

  const handleOpenConfig = async (): Promise<void> => {
    setIsOpeningFile(true)
    try {
      await window.api.config.openFile()
    } finally {
      setIsOpeningFile(false)
    }
  }

  const handleSelectDirectory = async (): Promise<void> => {
    try {
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
    } catch (err) {
      console.error('[OnboardingWizard] Error choosing directory:', err)
    }
  }

  const handleRefreshProviders = async (): Promise<void> => {
    setIsCheckingProviders(true)
    try {
      await loadAvailableModels()
    } finally {
      setIsCheckingProviders(false)
    }
  }

  const handleLaunch = async (): Promise<void> => {
    if (!name.trim() || !directory.trim() || !spec.trim()) return
    setError(null)
    setIsCreating(true)
    try {
      let finalSpec = spec.trim()
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
      if (fileContexts.size > 0) {
        const contextHeader =
          '--- REFERENCED FILES ---\n' +
          Array.from(fileContexts.entries())
            .map(([path, content]) => `@${path}:\n${content}`)
            .join('\n\n') +
          '\n\n--- TASK ---\n'
        finalSpec = contextHeader + finalSpec
      }
      await createPair({
        name: name.trim(),
        directory,
        spec: finalSpec,
        mentorModel,
        executorModel,
        branch,
        maxIterations: undefined
      })
      onComplete()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to create pair')
      setIsCreating(false)
    }
  }

  const canLaunch = useMemo(() => {
    return (
      providerSummary.isReady &&
      directory.trim().length > 0 &&
      name.trim().length > 0 &&
      spec.trim().length > 0 &&
      mentorModel.length > 0 &&
      executorModel.length > 0
    )
  }, [providerSummary.isReady, directory, name, spec, mentorModel, executorModel])

  return (
    <div className="fixed inset-0 z-50 flex flex-col bg-background grain-overlay">
      <div className="glass-toolbar app-drag shrink-0 border-b border-border/40 px-6 py-2.5 lg:px-8">
        <div className="mx-auto flex max-w-7xl items-center justify-between gap-4">
          <div className="flex min-w-0 items-center gap-3">
            <img src={appIcon} alt="The Pair" className="h-7 w-7 rounded-md object-contain" />
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold tracking-tight text-foreground">The Pair</span>
              <span className="shrink-0 rounded bg-blue-500 px-1.5 py-0.5 text-[10px] font-bold text-white">
                v{appVersion}
              </span>
              <span className="hidden text-[10px] uppercase tracking-[0.2em] text-muted-foreground sm:inline">
                · Setup Wizard
              </span>
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <button
              onClick={toggleTheme}
              className="app-no-drag rounded-lg p-1.5 text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground cursor-pointer"
              title={`Switch to ${theme === 'light' ? 'dark' : 'light'} mode`}
            >
              {theme === 'light' ? <Moon size={14} /> : <Sun size={14} />}
            </button>
          </div>
        </div>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto scrollbar-hide">
        <div className="mx-auto max-w-7xl px-6 py-5 lg:px-8 lg:py-6">
          <div className="flex flex-col gap-4">
            <WelcomeCard
              summary={providerSummary}
              loading={isCheckingProviders}
              onOpenConfig={handleOpenConfig}
              onRefresh={handleRefreshProviders}
              isOpening={isOpeningFile}
            />

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

            <div className="grid grid-cols-1 items-stretch gap-4 lg:grid-cols-3">
              <ModelCard
                availableModels={availableModels}
                mentorModel={mentorModel}
                executorModel={executorModel}
                onMentorChange={setMentorModel}
                onExecutorChange={setExecutorModel}
                isCompactLayout={isCompactLayout}
              />

              <DirectoryCard
                directory={directory}
                branch={branch}
                onBranchChange={setBranch}
                onSelectDirectory={handleSelectDirectory}
                isCompactLayout={isCompactLayout}
              />

              <TaskSpecCard
                name={name}
                spec={spec}
                directory={directory}
                onNameChange={setName}
                onSpecChange={setSpec}
                textareaRef={textareaRef}
                onFileSelect={handleFileSelect}
                onSkillSelect={handleSkillSelect}
                canLaunch={canLaunch}
                isCreating={isCreating}
                onLaunch={handleLaunch}
                error={error}
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

function WelcomeCard({
  summary,
  loading,
  onOpenConfig,
  onRefresh,
  isOpening
}: {
  summary: ProviderSetupSummary
  loading: boolean
  onOpenConfig: () => void
  onRefresh: () => void
  isOpening: boolean
}): React.ReactNode {
  const { t } = useTranslation()
  const healthState = summary.isReady ? 'ready' : summary.readyModelCount > 0 ? 'partial' : 'none'

  const healthConfig = {
    ready: {
      icon: <CheckCircle2 size={16} className="text-green-600 dark:text-green-400" />,
      label: t('onboarding.allReady'),
      description:
        summary.readyProviderLabels.length > 1
          ? t('onboarding.allReadyDesc_plural', {
              count: summary.readyModelCount,
              providers: summary.readyProviderLabels.length
            })
          : t('onboarding.allReadyDesc', {
              count: summary.readyModelCount,
              providers: summary.readyProviderLabels.length
            }),
      bgClass: 'bg-green-500/15 dark:bg-green-500/20',
      borderClass: 'border-green-500/25 dark:border-green-500/30',
      textClass: 'text-green-700 dark:text-green-300'
    },
    partial: {
      icon: <AlertCircle size={16} className="text-amber-600 dark:text-amber-400" />,
      label: t('onboarding.partialConfig'),
      description:
        summary.readyModelCount > 1
          ? t('onboarding.partialConfigDesc_plural', { count: summary.readyModelCount })
          : t('onboarding.partialConfigDesc', { count: summary.readyModelCount }),
      bgClass: 'bg-amber-500/15 dark:bg-amber-500/20',
      borderClass: 'border-amber-500/25 dark:border-amber-500/30',
      textClass: 'text-amber-700 dark:text-amber-300'
    },
    none: {
      icon: <AlertCircle size={16} className="text-red-600 dark:text-red-400" />,
      label: t('onboarding.noProviders'),
      description: t('onboarding.noProvidersDesc'),
      bgClass: 'bg-red-500/15 dark:bg-red-500/20',
      borderClass: 'border-red-500/25 dark:border-red-500/30',
      textClass: 'text-red-700 dark:text-red-300'
    }
  }

  const config = healthConfig[healthState]

  return (
    <GlassCard className="flex items-center gap-3 px-4 py-3">
      <div
        className={cn(
          'w-7 h-7 rounded-lg flex items-center justify-center shrink-0 border',
          config.bgClass,
          config.borderClass
        )}
      >
        <div className="scale-90">{config.icon}</div>
      </div>
      <div className="flex items-center gap-2 shrink-0">
        <span className="text-[9px] font-semibold uppercase tracking-[0.22em] text-muted-foreground">
          {t('onboarding.systemHealth')}
        </span>
        <span className="h-px w-4 bg-border/70" />
      </div>
      <div className="flex-1 min-w-0">
        <span className={cn('font-semibold text-xs', config.textClass)}>{config.label}</span>
        <span className="text-[11px] text-muted-foreground"> — {config.description}</span>
      </div>
      <div className="flex items-center gap-2 shrink-0">
        <GlassButton
          variant="ghost"
          size="sm"
          onClick={onRefresh}
          disabled={loading || isOpening}
          icon={<RefreshCw size={11} className={loading ? 'animate-spin' : ''} />}
        >
          {t('common.refresh')}
        </GlassButton>
        {!loading && (
          <GlassButton
            variant="secondary"
            size="sm"
            onClick={onOpenConfig}
            disabled={isOpening}
            icon={<ExternalLink size={11} />}
          >
            {isOpening ? t('onboarding.opening') : t('onboarding.openConfig')}
          </GlassButton>
        )}
      </div>
    </GlassCard>
  )
}

function DirectoryCard({
  directory,
  branch,
  onBranchChange,
  onSelectDirectory
}: {
  directory: string
  branch?: string
  onBranchChange: (branch: string | undefined) => void
  onSelectDirectory: () => void
  isCompactLayout?: boolean
}): React.ReactNode {
  const { t } = useTranslation()
  return (
    <GlassCard className="flex h-full flex-col p-5 space-y-3">
      <CardHeader
        eyebrow={t('common.workspace')}
        title={t('onboarding.chooseWorkspace')}
        description={t('onboarding.workspaceDesc')}
      />

      <GlassButton
        variant="secondary"
        onClick={onSelectDirectory}
        className="w-full h-auto flex flex-col items-center gap-2.5 py-5 flex-1"
      >
        <div className="flex h-10 w-10 items-center justify-center rounded-xl border border-border bg-muted">
          <FolderOpen size={20} className="text-foreground/40" />
        </div>
        <div className="text-center space-y-1">
          <p className="font-medium text-sm text-foreground">
            {directory ? t('onboarding.changeDirectory') : t('onboarding.selectFolder')}
          </p>
          {directory && (
            <p className="text-xs text-muted-foreground font-mono bg-muted px-3 py-1.5 rounded-lg inline-block">
              {directory}
            </p>
          )}
          {!directory && (
            <p className="text-xs text-muted-foreground">{t('onboarding.folderHint')}</p>
          )}
        </div>
      </GlassButton>

      {directory && <BranchPicker directory={directory} value={branch} onChange={onBranchChange} />}
    </GlassCard>
  )
}

function TaskSpecCard({
  name,
  spec,
  directory,
  onNameChange,
  onSpecChange,
  textareaRef,
  onFileSelect,
  onSkillSelect,
  canLaunch,
  isCreating,
  onLaunch,
  error
}: {
  name: string
  spec: string
  directory: string
  onNameChange: (v: string) => void
  onSpecChange: (v: string) => void
  textareaRef: React.RefObject<HTMLTextAreaElement | null>
  onFileSelect: (path: string, content: string) => void
  onSkillSelect: (skillName: string) => void
  canLaunch: boolean
  isCreating: boolean
  onLaunch: () => void
  error: string | null
}): React.ReactNode {
  const { t } = useTranslation()
  return (
    <GlassCard className="flex h-full flex-col p-5 space-y-3">
      <CardHeader
        eyebrow={t('common.task')}
        title={t('onboarding.taskSpec')}
        description={t('onboarding.taskSpecDesc')}
      />

      <div className="space-y-3 flex-1 flex flex-col">
        <div>
          <label className="mb-1 block text-xs font-medium text-foreground">
            {t('onboarding.pairName')}
          </label>
          <input
            type="text"
            value={name}
            onChange={(e) => onNameChange(e.target.value)}
            placeholder={t('onboarding.pairNamePlaceholder')}
            className="w-full rounded-xl glass-card px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-2 focus:ring-primary/20"
          />
        </div>

        <div className="relative flex-1 flex flex-col">
          <label className="mb-1 block text-xs font-medium text-foreground">
            {t('onboarding.taskDescription')}
          </label>
          <textarea
            ref={textareaRef}
            value={spec}
            onChange={(e) => onSpecChange(e.target.value)}
            placeholder={t('onboarding.taskPlaceholder')}
            rows={4}
            className="w-full resize-none rounded-xl glass-card px-3 py-2 text-sm leading-relaxed text-foreground placeholder:text-muted-foreground/50 focus:outline-none focus:ring-2 focus:ring-primary/20 flex-1 min-h-[120px]"
          />
          {directory && (
            <div className="absolute right-2 top-6 flex items-center gap-1">
              <SkillPicker projectDir={directory} onSelect={onSkillSelect} />
              <FileMention
                textareaRef={textareaRef}
                onChange={onSpecChange}
                directory={directory}
                onFileSelect={onFileSelect}
              />
            </div>
          )}
          <p className="mt-1 text-[10px] text-muted-foreground">
            {t('onboarding.charsHint', { count: spec.length })}
          </p>

          <div className="mt-3 pt-3 border-t border-border/40 flex flex-col gap-2">
            {error && (
              <div className="flex items-center gap-1.5 text-xs text-destructive">
                <AlertCircle size={13} />
                {error}
              </div>
            )}
            <button
              type="button"
              onClick={onLaunch}
              disabled={!canLaunch || isCreating}
              className={cn(
                'relative flex w-full items-center justify-center gap-2 overflow-hidden rounded-xl border px-6 py-2 text-sm font-semibold tracking-wide transition-all duration-200 cursor-pointer',
                'bg-gradient-to-b from-zinc-200 via-zinc-300 to-zinc-400',
                'dark:from-zinc-400 dark:via-zinc-600 dark:to-zinc-800',
                'border-zinc-400/60 dark:border-zinc-500/50',
                'text-zinc-800 dark:text-zinc-100',
                'shadow-[inset_0_1px_0_rgba(255,255,255,0.55),0_1px_3px_rgba(0,0,0,0.18)]',
                'dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.12),0_1px_3px_rgba(0,0,0,0.4)]',
                'hover:brightness-105 active:brightness-95 active:scale-[0.98]',
                (!canLaunch || isCreating) && 'cursor-not-allowed opacity-40'
              )}
            >
              <Rocket size={13} className="shrink-0" />
              {isCreating ? t('onboarding.launching') : t('onboarding.launch')}
            </button>
          </div>
        </div>
      </div>
    </GlassCard>
  )
}

function ModelCard({
  availableModels,
  mentorModel,
  executorModel,
  onMentorChange,
  onExecutorChange
}: {
  availableModels: AvailableModel[]
  mentorModel: string
  executorModel: string
  onMentorChange: (m: string) => void
  onExecutorChange: (m: string) => void
  isCompactLayout?: boolean
}): React.ReactNode {
  const { t } = useTranslation()
  return (
    <GlassCard className="flex h-full flex-col space-y-3 p-5">
      <CardHeader
        eyebrow={t('common.models')}
        title={t('onboarding.modelSelection')}
        description={t('onboarding.modelDesc')}
      />

      <div className="grid grid-cols-1 gap-3 flex-1">
        <ModelPicker
          value={mentorModel}
          models={availableModels}
          onChange={onMentorChange}
          role="mentor"
          variant="card"
        />
        <ModelPicker
          value={executorModel}
          models={availableModels}
          onChange={onExecutorChange}
          role="executor"
          variant="card"
          dropUp
        />
      </div>
    </GlassCard>
  )
}

function CardHeader({
  eyebrow,
  title,
  description
}: {
  eyebrow: string
  title: string
  description: string
}): React.ReactNode {
  return (
    <div className="space-y-1.5">
      <div className="flex items-center gap-2">
        <span className="text-[9px] font-semibold uppercase tracking-[0.22em] text-muted-foreground">
          {eyebrow}
        </span>
        <span className="h-px flex-1 bg-border/70" />
      </div>
      <div>
        <h3 className="text-xs font-semibold tracking-tight text-foreground">{title}</h3>
        <p className="text-[10px] leading-relaxed text-muted-foreground">{description}</p>
      </div>
    </div>
  )
}
