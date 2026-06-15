import React, { useState, useEffect } from 'react'
import {
  ChevronLeft,
  Eraser,
  Keyboard,
  Moon,
  Settings2,
  Sparkles,
  Sun,
  Volume2,
  VolumeX
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import { Pair } from '../store/usePairStore'
import { StatusBadge } from './StatusBadge'
import { GlassButton } from './ui/GlassButton'
import { UpdateControls } from './UpdateControls'
import { LanguageSwitcher } from './LanguageSwitcher'
import { isPairBusy } from '../lib/pairStatus'
import { setMuted, isMuted } from '../lib/sound'
import { modifierLabel } from '../lib/shortcuts'

interface AppChromeProps {
  selectedPair?: Pair | null
  readyModelCount: number
  totalModelCount: number
  modelsLoading?: boolean
  theme: 'light' | 'dark'
  onToggleTheme: () => void
  onNewTask: () => void
  onBack?: () => void
  onClearSession?: () => void
  onOpenSettings?: () => void
  onShowShortcuts?: () => void
}

function ChromeIconButton({
  onClick,
  title,
  children,
  testId
}: {
  onClick?: () => void
  title?: string
  children: React.ReactNode
  testId?: string
}): React.ReactNode {
  return (
    <button
      onClick={onClick}
      title={title}
      data-testid={testId}
      className="app-no-drag flex h-7 w-7 items-center justify-center rounded-sm border border-border bg-transparent text-muted-foreground transition-colors hover:bg-foreground/[0.06] hover:border-foreground/40 hover:text-foreground cursor-pointer"
    >
      {children}
    </button>
  )
}

export function AppChrome({
  selectedPair,
  readyModelCount,
  totalModelCount,
  modelsLoading = false,
  theme,
  onToggleTheme,
  onNewTask,
  onBack,
  onClearSession,
  onOpenSettings,
  onShowShortcuts
}: AppChromeProps): React.ReactNode {
  const { t } = useTranslation()
  const pairBusy = selectedPair ? isPairBusy(selectedPair.status) : false
  const hasMessages = (selectedPair?.messages.length ?? 0) > 0
  const canClearSession = Boolean(selectedPair) && !pairBusy && hasMessages
  const clearSessionTitle = !selectedPair
    ? undefined
    : pairBusy
      ? t('chrome.clearSessionDisabledBusy')
      : !hasMessages
        ? t('chrome.clearSessionDisabledEmpty')
        : t('chrome.clearSessionHint')
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
      .then((v: string) => setAppVersion(v))
      .catch((e: Error) => {
        console.error('[AppChrome] getVersion error:', e)
      })
  }, [])

  return (
    <div className="app-chrome shrink-0">
      <div className="app-drag flex items-center justify-between gap-3 px-4 py-2 font-mono">
        <div className="flex min-w-0 items-baseline gap-2">
          {selectedPair ? (
            <ChromeIconButton onClick={onBack} testId="chrome-back" title="back">
              <ChevronLeft size={14} />
            </ChromeIconButton>
          ) : (
            <span aria-hidden className="text-primary font-bold select-none">
              {'>_'}
            </span>
          )}

          <div className="min-w-0 flex flex-wrap items-baseline gap-2">
            <h1 className="truncate text-[12px] font-bold tracking-[0.06em] text-foreground">
              {selectedPair ? selectedPair.name : t('chrome.title')}
            </h1>
            <span className="text-[10px] uppercase tracking-[0.16em] text-muted-foreground-faint">
              v{appVersion ?? '…'}
            </span>
            <a
              href="https://timwuhaotian.github.io/"
              target="_blank"
              rel="noopener noreferrer"
              className="app-no-drag text-[10px] uppercase tracking-[0.14em] text-muted-foreground-faint hover:text-foreground/80 transition-colors"
              title="timwuhaotian"
            >
              · by timwuhaotian
            </a>
            {selectedPair && (
              <StatusBadge
                status={selectedPair.status}
                stalled={
                  (selectedPair.turn === 'mentor' &&
                    selectedPair.mentorActivity.phase === 'stalled') ||
                  (selectedPair.turn === 'executor' &&
                    selectedPair.executorActivity.phase === 'stalled')
                }
              />
            )}
            {(selectedPair?.pendingMentorModel || selectedPair?.pendingExecutorModel) && (
              <span className="inline-flex items-baseline gap-1 border border-state-running/40 bg-state-running/12 px-1.5 py-px text-[9px] uppercase tracking-[0.14em] state-running">
                → {t('chrome.modelsQueued')}
              </span>
            )}
            <p
              className={cn(
                'mt-0.5 w-full min-w-0 truncate text-[10px] text-muted-foreground',
                'sm:mt-0 sm:w-auto sm:basis-auto'
              )}
              title={selectedPair?.spec || selectedPair?.directory}
            >
              {selectedPair
                ? selectedPair.spec || selectedPair.directory
                : modelsLoading && totalModelCount === 0
                  ? t('chrome.detectingModels')
                  : t('chrome.modelsReady', { ready: readyModelCount, total: totalModelCount })}
            </p>
          </div>
        </div>

        <div className="app-no-drag flex shrink-0 items-center gap-1.5">
          {!selectedPair && <UpdateControls />}

          {selectedPair && (
            <>
              <GlassButton
                variant="secondary"
                size="sm"
                onClick={onOpenSettings}
                icon={<Settings2 size={11} />}
                data-testid="chrome-models"
              >
                {t('common.models')}
              </GlassButton>
              <GlassButton
                variant="secondary"
                size="sm"
                onClick={onClearSession}
                disabled={!canClearSession}
                title={clearSessionTitle}
                icon={<Eraser size={11} />}
                data-testid="chrome-clear-session"
              >
                {t('chrome.clearSession')}
              </GlassButton>
              <GlassButton
                variant="primary"
                size="sm"
                onClick={onNewTask}
                disabled={pairBusy}
                title={
                  pairBusy
                    ? t('chrome.newTaskDisabledBusy')
                    : `${t('chrome.newTaskHint')} · ${modifierLabel}N`
                }
                icon={<Sparkles size={11} />}
                data-testid="chrome-new-task"
              >
                {t('chrome.newTask')}
              </GlassButton>
            </>
          )}

          {onShowShortcuts && (
            <ChromeIconButton
              onClick={onShowShortcuts}
              title={`${t('shortcuts.title')} · ?`}
              testId="chrome-shortcuts"
            >
              <Keyboard size={13} />
            </ChromeIconButton>
          )}

          <ChromeIconButton
            onClick={toggleMute}
            title={soundMuted ? t('chrome.unmuteSounds') : t('chrome.muteSounds')}
            testId="chrome-mute-toggle"
          >
            {soundMuted ? <VolumeX size={13} /> : <Volume2 size={13} />}
          </ChromeIconButton>

          <LanguageSwitcher />

          <ChromeIconButton
            onClick={onToggleTheme}
            title={theme === 'light' ? t('chrome.switchToDark') : t('chrome.switchToLight')}
            testId="chrome-theme-toggle"
          >
            {theme === 'light' ? <Moon size={13} /> : <Sun size={13} />}
          </ChromeIconButton>
        </div>
      </div>
    </div>
  )
}
