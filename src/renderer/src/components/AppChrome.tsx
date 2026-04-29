import React, { useState, useEffect } from 'react'
import {
  ChevronLeft,
  Moon,
  Plus,
  Settings2,
  Sun,
  WandSparkles,
  Volume2,
  VolumeX
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Pair } from '../store/usePairStore'
import { StatusBadge } from './StatusBadge'
import { GlassButton } from './ui/GlassButton'
import { UpdateControls } from './UpdateControls'
import { LanguageSwitcher } from './LanguageSwitcher'
import { isPairBusy } from '../lib/pairStatus'
import { setMuted, isMuted } from '../lib/sound'

interface AppChromeProps {
  selectedPair?: Pair | null
  readyModelCount: number
  totalModelCount: number
  modelsLoading?: boolean
  theme: 'light' | 'dark'
  onToggleTheme: () => void
  onNewPair: () => void
  onBack?: () => void
  onAssignTask?: () => void
  onOpenSettings?: () => void
}

export function AppChrome({
  selectedPair,
  readyModelCount,
  totalModelCount,
  modelsLoading = false,
  theme,
  onToggleTheme,
  onNewPair,
  onBack,
  onAssignTask,
  onOpenSettings
}: AppChromeProps): React.ReactNode {
  const { t } = useTranslation()
  const pairBusy = selectedPair ? isPairBusy(selectedPair.status) : false
  const [soundMuted, setSoundMuted] = useState(isMuted())

  const toggleMute = (): void => {
    const next = !soundMuted
    setMuted(next)
    setSoundMuted(next)
  }

  const [appVersion, setAppVersion] = useState<string | null>(null)

  useEffect(() => {
    window.api?.config
      ?.getVersion?.()
      .then((v: string) => {
        setAppVersion(v)
      })
      .catch((e: Error) => {
        console.error('[AppChrome] getVersion error:', e)
      })
  }, [])

  return (
    <div className="app-chrome glass-toolbar relative shrink-0 border-b border-border/60">
      <div className="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-foreground/12 to-transparent" />
      <div className="app-drag relative flex items-center justify-between gap-4 px-5 py-2.5">
        <div className="flex min-w-0 items-center gap-3">
          {selectedPair ? (
            <button
              onClick={onBack}
              className="app-no-drag flex h-10 w-10 items-center justify-center rounded-2xl border border-border bg-muted/50 text-muted-foreground transition-all hover:bg-muted hover:text-foreground cursor-pointer"
              data-testid="chrome-back"
            >
              <ChevronLeft size={18} />
            </button>
          ) : (
            <div className="flex h-10 w-10 items-center justify-center rounded-2xl border border-border bg-gradient-to-br from-background via-muted/70 to-background shadow-sm">
              <span className="text-[10px] font-black tracking-[0.22em] text-foreground/70">
                TP
              </span>
            </div>
          )}

          <div className="min-w-0 flex flex-col gap-1">
            <div className="flex flex-wrap items-center gap-2">
              <h1 className="truncate text-sm font-semibold tracking-tight text-foreground">
                {selectedPair ? selectedPair.name : t('chrome.title')}
              </h1>
              <span className="rounded-full border border-primary/25 bg-primary/15 px-2 py-0.5 text-[10px] font-semibold text-primary">
                v{appVersion ?? '...'}
              </span>
              <a
                href="https://timwuhaotian.github.io/"
                target="_blank"
                rel="noopener noreferrer"
                className="app-no-drag group flex items-center gap-1.5 rounded-full border border-border/40 bg-gradient-to-r from-muted/30 to-muted/50 px-2.5 py-0.5 text-[11px] font-medium text-muted-foreground transition-colors duration-300"
                title="Visit timwuhaotian's homepage"
              >
                <span className="opacity-60 group-hover:opacity-100 transition-opacity duration-300">
                  by
                </span>
                <span className="font-semibold bg-gradient-to-r from-blue-600 to-purple-600 dark:from-blue-400 dark:to-purple-400 group-hover:from-blue-500 group-hover:to-purple-500 dark:group-hover:from-blue-300 dark:group-hover:to-purple-300 bg-clip-text text-transparent transition-all duration-300">
                  timwuhaotian
                </span>
              </a>
              {selectedPair ? (
                <StatusBadge
                  status={selectedPair.status}
                  stalled={
                    (selectedPair.turn === 'mentor' &&
                      selectedPair.mentorActivity.phase === 'stalled') ||
                    (selectedPair.turn === 'executor' &&
                      selectedPair.executorActivity.phase === 'stalled')
                  }
                />
              ) : null}
              {selectedPair?.pendingMentorModel || selectedPair?.pendingExecutorModel ? (
                <span className="rounded-full border border-amber-500/20 bg-amber-500/10 px-2 py-1 text-[10px] font-medium uppercase tracking-[0.16em] text-amber-700 dark:text-amber-300">
                  {t('chrome.modelsQueued')}
                </span>
              ) : null}
            </div>
            <p className="truncate text-xs text-muted-foreground">
              {selectedPair
                ? selectedPair.spec || selectedPair.directory
                : modelsLoading && totalModelCount === 0
                  ? t('chrome.detectingModels')
                  : t('chrome.modelsReady', { ready: readyModelCount, total: totalModelCount })}
            </p>
          </div>
        </div>

        <div className="app-no-drag flex shrink-0 items-center gap-2">
          {!selectedPair ? <UpdateControls /> : null}

          {selectedPair && (
            <>
              <GlassButton
                variant="secondary"
                size="sm"
                onClick={onOpenSettings}
                icon={<Settings2 size={13} />}
                data-testid="chrome-models"
              >
                {t('common.models')}
              </GlassButton>
              <GlassButton
                variant="primary"
                size="sm"
                onClick={onAssignTask}
                disabled={pairBusy}
                icon={<WandSparkles size={13} />}
                data-testid="chrome-new-task"
              >
                {t('chrome.newTask')}
              </GlassButton>
            </>
          )}

          <GlassButton
            variant="secondary"
            size="sm"
            onClick={onNewPair}
            icon={<Plus size={13} />}
            data-testid="chrome-new-pair"
          >
            {t('chrome.newPair')}
          </GlassButton>

          <button
            onClick={toggleMute}
            className="flex h-10 w-10 items-center justify-center rounded-2xl border border-border bg-muted/40 text-muted-foreground transition-all hover:bg-muted hover:text-foreground cursor-pointer"
            title={soundMuted ? t('chrome.unmuteSounds') : t('chrome.muteSounds')}
            data-testid="chrome-mute-toggle"
          >
            {soundMuted ? <VolumeX size={16} /> : <Volume2 size={16} />}
          </button>

          <LanguageSwitcher />

          <button
            onClick={onToggleTheme}
            className="flex h-10 w-10 items-center justify-center rounded-2xl border border-border bg-muted/40 text-muted-foreground transition-all hover:bg-muted hover:text-foreground cursor-pointer"
            title={theme === 'light' ? t('chrome.switchToDark') : t('chrome.switchToLight')}
            data-testid="chrome-theme-toggle"
          >
            {theme === 'light' ? <Moon size={16} /> : <Sun size={16} />}
          </button>
        </div>
      </div>
    </div>
  )
}
