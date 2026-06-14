import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import {
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
import type { AvailableModel, PairPreset, DetectedProviderProfile } from '../types'
import { GlassButton } from './ui/GlassButton'
import { ModelPicker } from './ModelPicker'
import { FileMention } from './FileMention'
import { SkillPicker } from './SkillPicker'
import { BranchPicker } from './BranchPicker'
import { PresetPicker } from './PresetPicker'
import { getPreferredPairModelSelection } from '../lib/modelPreferences'
import { derivePairNameFromDirectory } from '../lib/workspace'
import { shouldUseCompactOnboardingLayout } from '../lib/onboardingLayout'
import {
  buildProviderSetupSummary,
  buildProviderSetupHints
} from '../lib/providerSetup'
import type { ProviderSetupSummary } from '../lib/providerSetup'
import type { ProviderSetupHint } from '../types'
import { buildSpecFromPreset, stripTemplate } from '../lib/presetUtils'
import { usePresets } from '../lib/usePresets'
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
  const [providerProfiles, setProviderProfiles] = useState<DetectedProviderProfile[]>([])

  // Build per-provider hints. We use two data sources:
  // 1. availableModels → model counts + ready/auth classification
  // 2. providerProfiles → covers providers that are installed but have zero models
  //    in the catalog (e.g. OpenCode filters out unavailable models entirely).
  //    Also provides authoritative loginCommand/installUrl (important for Gemini
  //    where it differs between agy and legacy).
  const providerHints = useMemo<ProviderSetupHint[]>(() => {
    const modelHints = buildProviderSetupHints(availableModels)
    const seenKinds = new Set(modelHints.map((h) => h.kind))

    // Add providers from profiles that have no models in the catalog
    for (const profile of providerProfiles) {
      if (seenKinds.has(profile.kind)) continue
      // Only include if installed (don't show profiles for CLIs not even on the system)
      if (!profile.installed) continue
      seenKinds.add(profile.kind)
      modelHints.push({
        kind: profile.kind,
        label:
          profile.kind === 'claude'
            ? 'Claude Code'
            : profile.kind === 'codex'
              ? 'Codex'
              : profile.kind === 'gemini'
                ? 'Gemini CLI'
                : 'OpenCode',
        installed: true,
        authenticated: profile.authenticated,
        readyModelCount: 0,
        loginCommand: profile.loginCommand,
        installUrl: profile.installUrl
      })
    }

    // Overlay backend-provided loginCommand/installUrl for existing hints too
    const merged = modelHints.map((hint) => {
      const profile = providerProfiles.find((p) => p.kind === hint.kind)
      return {
        ...hint,
        loginCommand: profile?.loginCommand ?? hint.loginCommand,
        installUrl: profile?.installUrl ?? hint.installUrl
      }
    })

    // Re-sort: ready first, then auth-missing, then cli-missing
    return merged.sort((a, b) => {
      const rank = (ready: number, installed: boolean): number => {
        if (ready > 0) return 0
        if (installed) return 1
        return 2
      }
      return rank(a.readyModelCount, a.installed) - rank(b.readyModelCount, b.installed)
    })
  }, [availableModels, providerProfiles])

  useEffect(() => {
    if (availableModels.length > 0) {
      setIsCheckingProviders(false)
      // Also fetch provider profiles (for login commands) if we haven't yet
      if (providerProfiles.length === 0) {
        void window.api?.config?.getProviders?.().then((data: unknown) => {
          if (Array.isArray(data)) {
            setProviderProfiles(data as DetectedProviderProfile[])
          }
        })
      }
      return
    }
    let cancelled = false
    setIsCheckingProviders(true)
    void (async () => {
      await loadAvailableModels()
      try {
        const profiles = await window.api?.config?.getProviders?.()
        if (!cancelled && Array.isArray(profiles)) {
          setProviderProfiles(profiles as DetectedProviderProfile[])
        }
      } catch {
        // non-critical — hints will use static fallbacks
      }
      if (!cancelled) {
        setIsCheckingProviders(false)
      }
    })()

    return () => {
      cancelled = true
    }
  }, [loadAvailableModels, availableModels.length, providerProfiles.length])

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
      try {
        const profiles = await window.api?.config?.getProviders?.()
        if (Array.isArray(profiles)) {
          setProviderProfiles(profiles as DetectedProviderProfile[])
        }
      } catch {
        // non-critical
      }
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
    <div className="fixed inset-0 z-50 flex flex-col bg-background font-mono">
      <div className="app-chrome app-drag shrink-0 px-5 py-2 lg:px-8">
        <div className="mx-auto flex max-w-7xl items-baseline justify-between gap-4">
          <div className="flex min-w-0 items-baseline gap-2">
            <img
              src={appIcon}
              alt="The Pair"
              className="h-5 w-5 rounded-sm object-contain translate-y-1"
            />
            <span aria-hidden className="text-foreground/70 select-none">
              {'>_'}
            </span>
            <span className="text-[12px] font-bold tracking-[0.06em] text-foreground">
              the-pair
            </span>
            <span className="text-[10px] uppercase tracking-[0.16em] text-muted-foreground-faint">
              v{appVersion}
            </span>
            <span className="hidden text-[10px] uppercase tracking-[0.16em] text-muted-foreground-faint sm:inline">
              · setup wizard
            </span>
          </div>
          <div className="flex shrink-0 items-baseline gap-2">
            <button
              onClick={toggleTheme}
              className="app-no-drag h-7 w-7 inline-flex items-center justify-center rounded-sm border border-border bg-transparent text-muted-foreground transition-colors hover:bg-foreground/[0.06] hover:border-foreground/40 hover:text-foreground cursor-pointer"
              title={`switch to ${theme === 'light' ? 'dark' : 'light'} mode`}
            >
              {theme === 'light' ? <Moon size={12} /> : <Sun size={12} />}
            </button>
          </div>
        </div>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto scrollbar-thin">
        <div className="mx-auto max-w-7xl px-6 py-5 lg:px-8 lg:py-6">
          <div className="flex flex-col gap-3">
            <WelcomeCard
              summary={providerSummary}
              hints={providerHints}
              loading={isCheckingProviders}
              onOpenConfig={handleOpenConfig}
              onRefresh={handleRefreshProviders}
              isOpening={isOpeningFile}
            />

            <div>
              <div className="mb-1 flex items-baseline gap-1.5 text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
                <Sparkles size={10} className="state-running translate-y-px" />
                <span>workflow preset</span>
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
  hints,
  loading,
  onOpenConfig,
  onRefresh,
  isOpening
}: {
  summary: ProviderSetupSummary
  hints: ProviderSetupHint[]
  loading: boolean
  onOpenConfig: () => void
  onRefresh: () => void
  isOpening: boolean
}): React.ReactNode {
  const { t } = useTranslation()
  const [loginStatus, setLoginStatus] = useState<
    Record<string, 'launching' | 'launched' | 'failed'>
  >({})

  const healthState = summary.isReady ? 'ready' : summary.readyModelCount > 0 ? 'partial' : 'none'

  const healthConfig = {
    ready: {
      glyph: '✓',
      tone: 'state-done',
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
            })
    },
    partial: {
      glyph: '!',
      tone: 'state-running',
      label: t('onboarding.partialConfig'),
      description:
        summary.readyModelCount > 1
          ? t('onboarding.partialConfigDesc_plural', { count: summary.readyModelCount })
          : t('onboarding.partialConfigDesc', { count: summary.readyModelCount })
    },
    none: {
      glyph: '✗',
      tone: 'state-error',
      label: t('onboarding.noProviders'),
      description: t('onboarding.noProvidersDesc')
    }
  }

  const config = healthConfig[healthState]

  const handleSignIn = async (hint: ProviderSetupHint): Promise<void> => {
    if (!hint.loginCommand) return
    setLoginStatus((prev) => ({ ...prev, [hint.kind]: 'launching' }))
    try {
      await window.api?.config?.launchLogin?.(hint.loginCommand)
      setLoginStatus((prev) => ({ ...prev, [hint.kind]: 'launched' }))
    } catch {
      setLoginStatus((prev) => ({ ...prev, [hint.kind]: 'failed' }))
    }
  }

  const handleInstall = (hint: ProviderSetupHint): void => {
    if (hint.installUrl) {
      window.open(hint.installUrl, '_blank')
    }
  }

  return (
    <div className="border border-border bg-background/40 px-3 py-2 font-mono text-[12px]">
      {/* Top row: summary line + action buttons */}
      <div className="flex items-baseline gap-3">
        <span aria-hidden className={cn('w-[1ch] tabular-nums select-none', config.tone)}>
          {config.glyph}
        </span>
        <span className="text-[10px] uppercase tracking-[0.18em] text-muted-foreground shrink-0">
          {t('onboarding.systemHealth')}
        </span>
        <span className="text-muted-foreground-faint shrink-0">·</span>
        <div className="flex-1 min-w-0">
          <span className={cn('font-bold', config.tone)}>{config.label}</span>
          <span className="text-muted-foreground text-[11px]"> — {config.description}</span>
        </div>
        <div className="flex items-center gap-1.5 shrink-0">
          <GlassButton
            variant="ghost"
            size="sm"
            onClick={onRefresh}
            disabled={loading || isOpening}
            icon={<RefreshCw size={10} className={loading ? 'animate-spin' : ''} />}
          >
            {t('common.refresh')}
          </GlassButton>
          {!loading && (
            <GlassButton
              variant="secondary"
              size="sm"
              onClick={onOpenConfig}
              disabled={isOpening}
              icon={<ExternalLink size={10} />}
            >
              {isOpening ? t('onboarding.opening') : t('onboarding.openConfig')}
            </GlassButton>
          )}
        </div>
      </div>

      {/* Per-provider breakdown */}
      {hints.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 border-t border-border/50 pt-2">
          {hints.map((hint) => {
            const status = loginStatus[hint.kind]
            const isReady = hint.readyModelCount > 0
            const isAuthMissing = hint.installed && !hint.authenticated
            const isCliMissing = !hint.installed
            const isRuntimeUnsupported = hint.installed && hint.authenticated && !isReady
            const tone = isReady
              ? 'state-done'
              : isAuthMissing
                ? 'state-running'
                : isRuntimeUnsupported
                  ? 'state-running'
                  : 'state-error'
            const glyph = isReady ? '✓' : isAuthMissing || isRuntimeUnsupported ? '⚠' : '✗'

            return (
              <div key={hint.kind} className="flex items-center gap-1.5 text-[11px]">
                <span aria-hidden className={cn('select-none', tone)}>
                  {glyph}
                </span>
                <span className="text-foreground/80 font-medium">{hint.label}</span>
                {isReady ? (
                  <span className="text-muted-foreground">
                    {hint.readyModelCount > 1
                      ? t('providers.ready', { count: hint.readyModelCount })
                      : t('providers.ready_one', { count: hint.readyModelCount })}
                  </span>
                ) : isAuthMissing ? (
                  <>
                    <span className="text-muted-foreground">{t('providers.notSignedIn')}</span>
                    {hint.loginCommand && (
                      <button
                        type="button"
                        onClick={() => void handleSignIn(hint)}
                        disabled={status === 'launching'}
                        className="text-[10px] text-blue-500 hover:text-blue-600 dark:text-blue-400 dark:hover:text-blue-300 underline-offset-2 hover:underline disabled:opacity-50 cursor-pointer"
                      >
                        {t('providers.signIn')}
                      </button>
                    )}
                  </>
                ) : isCliMissing ? (
                  <>
                    <span className="text-muted-foreground">{t('providers.notInstalled')}</span>
                    {hint.installUrl && (
                      <button
                        type="button"
                        onClick={() => handleInstall(hint)}
                        className="text-[10px] text-blue-500 hover:text-blue-600 dark:text-blue-400 dark:hover:text-blue-300 underline-offset-2 hover:underline cursor-pointer"
                      >
                        {t('providers.install')} ↗
                      </button>
                    )}
                  </>
                ) : null}
                {status === 'launched' && !isReady && (
                  <span className="text-[10px] text-muted-foreground italic">
                    {t('providers.loginLaunched')}
                  </span>
                )}
                {status === 'failed' && hint.loginCommand && (
                  <span className="text-[10px] text-red-500 dark:text-red-400">
                    {t('providers.loginFailed')}{' '}
                    <code className="bg-foreground/[0.06] px-1 rounded select-all">
                      {hint.loginCommand}
                    </code>
                  </span>
                )}
              </div>
            )
          })}
        </div>
      )}
    </div>
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
    <div className="flex h-full flex-col border border-border rounded-sm p-4 font-mono space-y-3 bg-background/40">
      <CardHeader
        eyebrow={t('common.workspace')}
        title={t('onboarding.chooseWorkspace')}
        description={t('onboarding.workspaceDesc')}
      />

      <button
        type="button"
        onClick={onSelectDirectory}
        className="flex flex-col items-start gap-2 border border-dashed border-border bg-background/40 px-3 py-3 text-left hover:border-foreground/40 hover:bg-foreground/[0.04] transition-colors cursor-pointer rounded-sm flex-1"
      >
        <div className="flex items-baseline gap-2">
          <FolderOpen size={12} className="text-muted-foreground translate-y-px" />
          <span className="text-[12px] text-foreground/90">
            {directory
              ? `▸ ${t('onboarding.changeDirectory').toLowerCase()}`
              : `▸ ${t('onboarding.selectFolder').toLowerCase()}`}
          </span>
        </div>
        {directory ? (
          <p className="text-[11px] text-foreground/75 [overflow-wrap:anywhere]" title={directory}>
            {directory}
          </p>
        ) : (
          <p className="text-[10px] text-muted-foreground-faint">· {t('onboarding.folderHint')}</p>
        )}
      </button>

      {directory && <BranchPicker directory={directory} value={branch} onChange={onBranchChange} />}
    </div>
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
    <div className="flex h-full flex-col border border-border rounded-sm p-4 font-mono space-y-3 bg-background/40">
      <CardHeader
        eyebrow={t('common.task')}
        title={t('onboarding.taskSpec')}
        description={t('onboarding.taskSpecDesc')}
      />

      <div className="space-y-2 flex-1 flex flex-col">
        <div className="flex flex-col gap-1">
          <label className="text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
            {t('onboarding.pairName')}
          </label>
          <input
            type="text"
            value={name}
            onChange={(e) => onNameChange(e.target.value)}
            placeholder={t('onboarding.pairNamePlaceholder')}
            className="w-full px-2 py-1.5 bg-background border border-border text-[12px] text-foreground placeholder:text-muted-foreground-faint focus:outline-none focus:border-foreground/60 rounded-sm"
          />
        </div>

        <div className="relative flex-1 flex flex-col gap-1">
          <label className="text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
            {t('onboarding.taskDescription')}
          </label>
          <textarea
            ref={textareaRef}
            value={spec}
            onChange={(e) => onSpecChange(e.target.value)}
            placeholder={t('onboarding.taskPlaceholder')}
            rows={4}
            className="w-full resize-none px-2 py-1.5 bg-background border border-border text-[12px] text-foreground placeholder:text-muted-foreground-faint leading-relaxed focus:outline-none focus:border-foreground/60 rounded-sm flex-1 min-h-[120px]"
          />
          {directory && (
            <div className="absolute right-1 top-[26px] flex items-center gap-0.5">
              <SkillPicker projectDir={directory} onSelect={onSkillSelect} />
              <FileMention
                textareaRef={textareaRef}
                onChange={onSpecChange}
                directory={directory}
                onFileSelect={onFileSelect}
              />
            </div>
          )}
          <p className="text-[10px] text-muted-foreground-faint">
            · {t('onboarding.charsHint', { count: spec.length })}
          </p>

          <div className="mt-2 pt-2 border-t border-border flex flex-col gap-1.5">
            {error && (
              <div className="flex items-baseline gap-1.5 text-[11px] state-error">
                <AlertCircle size={11} className="translate-y-px" />
                <span>✗ {error}</span>
              </div>
            )}
            <button
              type="button"
              onClick={onLaunch}
              disabled={!canLaunch || isCreating}
              className={cn(
                'w-full inline-flex items-center justify-center gap-2 border border-foreground bg-foreground text-background px-4 py-2 text-[12px] font-bold uppercase tracking-[0.12em] rounded-sm transition-colors duration-150 cursor-pointer',
                'hover:bg-foreground/90',
                (!canLaunch || isCreating) && 'cursor-not-allowed opacity-40'
              )}
            >
              <Rocket size={12} className="shrink-0" />▸{' '}
              {isCreating
                ? t('onboarding.launching').toLowerCase()
                : t('onboarding.launch').toLowerCase()}
            </button>
          </div>
        </div>
      </div>
    </div>
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
    <div className="flex h-full flex-col border border-border rounded-sm p-4 font-mono space-y-3 bg-background/40">
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
    </div>
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
    <div className="space-y-1">
      <div className="flex items-baseline gap-2 text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
        <span>── {eyebrow.toLowerCase()}</span>
        <span className="flex-1 tty-divider" />
      </div>
      <div>
        <h3 className="text-[12px] font-bold text-foreground/90">{title}</h3>
        <p className="text-[10px] leading-relaxed text-muted-foreground">{description}</p>
      </div>
    </div>
  )
}
