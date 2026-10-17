import React, { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { useTranslation } from 'react-i18next'
import { usePairStore, type Pair } from './store/usePairStore'
import { useThemeStore } from './store/useThemeStore'
import { useUpdateStore } from './store/useUpdateStore'
import './i18n'
import { AppChrome } from './components/AppChrome'
import { AppShell, type ShellSection } from './components/AppShell'
import { PairsHub } from './components/PairsHub'
import { ActivityView, InsightsView } from './components/ShellViews'
import { BootSplash } from './components/BootSplash'
import { ConfirmModal } from './components/ui/ConfirmModal'
import { UpdateNotification } from './components/UpdateNotification'
import { isSelectableForPairExecution } from './lib/modelPreferences'
import { ErrorBoundary } from './components/ErrorBoundary'
import { canStartUpdateCheck, shouldShowUpdateModal } from './components/updateView'
import { isPairActive, isPairBusy } from './lib/pairStatus'
import { extractErrorMessage } from './lib/utils'
import { Dashboard } from './components/Dashboard'
import { preloadSounds } from './lib/sound'
import { useShortcuts } from './hooks/useShortcuts'
import { ShortcutsModal } from './components/ShortcutsModal'
import { canOpenConsoleSearch, type ShortcutDef } from './lib/shortcuts'
import {
  buildPaletteActions,
  buildPairJumpActions,
  type PaletteContext
} from './lib/paletteActions'

const CommandPalette = lazy(() =>
  import('./components/CommandPalette').then(({ CommandPalette }) => ({
    default: CommandPalette
  }))
)

const CreatePairModal = lazy(() =>
  import('./components/CreatePairModal').then(({ CreatePairModal }) => ({
    default: CreatePairModal
  }))
)
const AssignTaskModal = lazy(() =>
  import('./components/AssignTaskModal').then(({ AssignTaskModal }) => ({
    default: AssignTaskModal
  }))
)
const PairSettingsModal = lazy(() =>
  import('./components/PairSettingsModal').then(({ PairSettingsModal }) => ({
    default: PairSettingsModal
  }))
)

const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/.test(navigator.platform)
const cmdKey = isMac ? 'meta' : 'ctrl'
/**
 * Dev/QA escape hatch — `?splash=hold` keeps the splash mounted, while
 * `?splash=<ms>` overrides the minimum visible duration. Production users
 * never hit this branch (no query string), so the splash auto-fades after
 * stores hydrate.
 */
function readSplashOverride(): { hold: boolean; minMs: number } {
  if (typeof window === 'undefined') return { hold: false, minMs: 700 }
  try {
    const params = new URLSearchParams(window.location.search)
    const raw = params.get('splash')
    if (!raw) return { hold: false, minMs: 700 }
    if (raw === 'hold') return { hold: true, minMs: 700 }
    const parsed = Number.parseInt(raw, 10)
    if (Number.isFinite(parsed) && parsed > 0) return { hold: false, minMs: parsed }
  } catch {
    /* noop */
  }
  return { hold: false, minMs: 700 }
}

const SPLASH_OVERRIDE = readSplashOverride()

function queueStartupUpdateCheck(callback: () => void): () => void {
  let frameId: number | null = null
  let timeoutId: number | null = null

  const schedule = (): void => {
    timeoutId = window.setTimeout(callback, 0)
  }

  if ('requestAnimationFrame' in window) {
    frameId = window.requestAnimationFrame(schedule)
  } else {
    schedule()
  }

  return () => {
    if (frameId !== null) window.cancelAnimationFrame(frameId)
    if (timeoutId !== null) window.clearTimeout(timeoutId)
  }
}

function App(): React.ReactNode {
  const { t } = useTranslation()
  const [selectedPairId, setSelectedPairId] = useState<string | null>(null)
  const [shellSection, setShellSection] = useState<ShellSection>('pairs')
  const [shellCollapsed, setShellCollapsed] = useState(false)
  const [isCreatePairOpen, setIsCreatePairOpen] = useState(false)
  const [isAssignTaskOpen, setIsAssignTaskOpen] = useState(false)
  const [isPairSettingsOpen, setIsPairSettingsOpen] = useState(false)
  const [isShortcutsOpen, setIsShortcutsOpen] = useState(false)
  const [isPaletteOpen, setIsPaletteOpen] = useState(false)
  const [hasPaletteLoaded, setHasPaletteLoaded] = useState(false)
  // Incremented on every open so the palette remounts fresh (empty query,
  // selection at top) without reset effects inside the component.
  const [paletteEpoch, setPaletteEpoch] = useState(0)
  const openPalette = useCallback((): void => {
    setPaletteEpoch((epoch) => epoch + 1)
    setIsPaletteOpen(true)
  }, [])

  // The Cmd+K shortcut memo below reads open state without re-subscribing the
  // global key handler on every palette open/close.
  const isPaletteOpenRef = useRef(isPaletteOpen)
  isPaletteOpenRef.current = isPaletteOpen
  const [hasCreatePairModalLoaded, setHasCreatePairModalLoaded] = useState(false)
  const [hasAssignTaskModalLoaded, setHasAssignTaskModalLoaded] = useState(false)
  const [hasPairSettingsModalLoaded, setHasPairSettingsModalLoaded] = useState(false)
  const [pairsLoaded, setPairsLoaded] = useState(false)
  const [deletingPairId, setDeletingPairId] = useState<string | null>(null)
  const [pendingDeletePair, setPendingDeletePair] = useState<Pair | null>(null)
  const [pendingClearPair, setPendingClearPair] = useState<Pair | null>(null)
  const splashHoldVisible = SPLASH_OVERRIDE.hold
  const splashHoldDuration = SPLASH_OVERRIDE.minMs

  const pairs = usePairStore((state) => state.pairs)
  const availableModels = usePairStore((state) => state.availableModels)
  const modelsLoading = usePairStore((state) => state.isLoadingModels)
  const loadAvailableModels = usePairStore((state) => state.loadAvailableModels)
  const loadAllPairs = usePairStore((state) => state.loadAllPairs)
  const flushSnapshots = usePairStore((state) => state.flushSnapshots)
  const initMessageListener = usePairStore((state) => state.initMessageListener)
  const pausePair = usePairStore((state) => state.pausePair)
  const resumePair = usePairStore((state) => state.resumePair)
  const deletePair = usePairStore((state) => state.deletePair)
  const setRestoringSpec = usePairStore((state) => state.setRestoringSpec)
  const setViewingRunId = usePairStore((state) => state.setViewingRunId)
  const setMessages = usePairStore((state) => state.setMessages)

  const theme = useThemeStore((state) => state.theme)
  const toggleTheme = useThemeStore((state) => state.toggleTheme)

  const setPhase = useUpdateStore((state) => state.setPhase)
  const setVersion = useUpdateStore((state) => state.setVersion)
  const setMessage = useUpdateStore((state) => state.setMessage)
  const setReleaseBody = useUpdateStore((state) => state.setReleaseBody)
  const setUpdate = useUpdateStore((state) => state.setUpdate)
  const setShowModal = useUpdateStore((state) => state.setShowModal)
  const updateModalVisible = useUpdateStore((state) =>
    shouldShowUpdateModal(state.showModal, state.phase)
  )
  const displayToast = useUpdateStore((state) => state.displayToast)
  const updateRef = useRef<Update | null>(null)

  useEffect(() => {
    let unlisten: (() => void) | undefined
    let active = true

    const performUpdateCheck = async (): Promise<void> => {
      // A check while an update installs would close the Update being installed.
      if (!canStartUpdateCheck(useUpdateStore.getState().phase)) return
      setPhase('checking')
      setMessage('Checking for updates...')

      const TIMEOUT = Symbol('timeout')
      const timeoutPromise = new Promise<typeof TIMEOUT>((resolve) => {
        setTimeout(() => resolve(TIMEOUT), 30000)
      })

      try {
        if (updateRef.current) {
          await updateRef.current.close().catch(() => {})
          updateRef.current = null
        }

        const update = await Promise.race([check(), timeoutPromise])

        if (update === TIMEOUT) {
          console.error('[Updater] Check timed out after 30 seconds')
          setMessage('Update check timed out')
          setPhase('error')
          displayToast('Update check timed out', 'error')
          return
        }

        if (!update) {
          setVersion(null)
          setMessage('You are up to date')
          setPhase('up-to-date')
          return
        }

        updateRef.current = update
        setUpdate(update)
        setVersion(update.version)
        setReleaseBody(update.body || null)
        setMessage(`Version ${update.version} is available`)
        setPhase('available')
        setShowModal(true)
      } catch (error) {
        console.error('[Updater] Check failed:', error)
        const message = error instanceof Error ? error.message : 'Unable to check for updates'
        setMessage(message)
        setPhase('error')
        displayToast(message, 'error')
      }
    }

    void listen('app:update:check', () => {
      void performUpdateCheck()
    }).then((cleanup) => {
      // If the effect was already torn down before listen() resolved, dispose
      // the handler immediately so it can't leak into the next mount.
      if (active) {
        unlisten = cleanup
      } else {
        cleanup()
      }
    })

    const cancelQueuedUpdateCheck = import.meta.env.PROD
      ? queueStartupUpdateCheck(() => {
          void performUpdateCheck()
        })
      : undefined

    return () => {
      active = false
      cancelQueuedUpdateCheck?.()
      unlisten?.()
      if (updateRef.current) {
        void updateRef.current.close().catch(() => {})
      }
    }
  }, [setPhase, setVersion, setMessage, setReleaseBody, setUpdate, setShowModal, displayToast])

  useEffect(() => {
    preloadSounds()
    let cancelled = false
    const init = async (): Promise<void> => {
      initMessageListener()
      void loadAvailableModels()
      await loadAllPairs()
      if (!cancelled) setPairsLoaded(true)
    }
    void init()
    return () => {
      cancelled = true
    }
  }, [loadAvailableModels, initMessageListener, loadAllPairs])

  useEffect(() => {
    document.documentElement.classList.toggle('dark', theme === 'dark')
  }, [theme])

  useEffect(() => {
    if (isCreatePairOpen) setHasCreatePairModalLoaded(true)
  }, [isCreatePairOpen])

  useEffect(() => {
    if (isAssignTaskOpen) setHasAssignTaskModalLoaded(true)
  }, [isAssignTaskOpen])

  useEffect(() => {
    if (isPairSettingsOpen) setHasPairSettingsModalLoaded(true)
  }, [isPairSettingsOpen])

  useEffect(() => {
    if (isPaletteOpen) setHasPaletteLoaded(true)
  }, [isPaletteOpen])

  useEffect(() => {
    // Snapshot save is async + fire-and-forget; on a true app close we have no
    // way to await it. Listen on multiple lifecycle events so we get a save in
    // before the window dies even if the user closes via OS rather than menu:
    //   - visibilitychange (hidden): user minimized / switched workspace → checkpoint
    //   - pagehide: webview is being torn down → last-chance flush
    //   - beforeunload: classic reload / close path
    const flush = (): void => {
      void flushSnapshots()
    }
    const handleVisibilityChange = (): void => {
      if (document.visibilityState === 'hidden') flush()
    }

    window.addEventListener('beforeunload', flush)
    window.addEventListener('pagehide', flush)
    document.addEventListener('visibilitychange', handleVisibilityChange)
    return () => {
      window.removeEventListener('beforeunload', flush)
      window.removeEventListener('pagehide', flush)
      document.removeEventListener('visibilitychange', handleVisibilityChange)
    }
  }, [flushSnapshots])

  const selectedPair = pairs.find((p) => p.id === selectedPairId) ?? null
  const readyModelCount = availableModels.filter((model) =>
    isSelectableForPairExecution(model)
  ).length
  const shouldRenderCreatePairModal = hasCreatePairModalLoaded || isCreatePairOpen
  const shouldRenderAssignTaskModal = hasAssignTaskModalLoaded || isAssignTaskOpen
  const shouldRenderPairSettingsModal = hasPairSettingsModalLoaded || isPairSettingsOpen
  const shouldRenderPalette = hasPaletteLoaded || isPaletteOpen

  // 如果选中了一个不存在的 pair，重置选择
  useEffect(() => {
    if (selectedPairId && !selectedPair) {
      setSelectedPairId(null)
      setViewingRunId(null)
    }
  }, [selectedPairId, selectedPair, setViewingRunId])

  const handlePauseSelectedPair = useCallback(async (): Promise<void> => {
    if (!selectedPair || !isPairActive(selectedPair.status)) return

    try {
      await pausePair(selectedPair.id)
    } catch (error) {
      console.error('[App] Failed to pause pair:', error)
    }
  }, [selectedPair, pausePair])

  const handleResumeSelectedPair = useCallback(async (): Promise<void> => {
    if (!selectedPair || selectedPair.status !== 'Paused') return

    try {
      await resumePair(selectedPair.id)
    } catch (error) {
      console.error('[App] Failed to resume pair:', error)
    }
  }, [selectedPair, resumePair])

  // Open the "New Task" modal for the currently selected pair. Clears any
  // restore state so the modal treats this as a fresh task, not a restore.
  const handleNewTask = useCallback((): void => {
    if (!selectedPair || isPairBusy(selectedPair.status)) return
    setRestoringSpec(null)
    setIsAssignTaskOpen(true)
  }, [selectedPair, setRestoringSpec])

  // viewingRunId is global in the store while run ids are per pair — leave any
  // archived-run view when switching pairs or going back to the hub.
  const selectPair = useCallback(
    (pairId: string | null): void => {
      if (pairId !== selectedPairId) setViewingRunId(null)
      setSelectedPairId(pairId)
    },
    [selectedPairId, setViewingRunId]
  )

  const goHub = useCallback((): void => {
    selectPair(null)
  }, [selectPair])

  const selectSection = useCallback(
    (next: ShellSection): void => {
      setShellSection(next)
      // Switching sections always lands on the hub; the detail view is
      // entered by picking a pair card.
      if (selectedPairId !== null) setViewingRunId(null)
      setSelectedPairId(null)
    },
    [selectedPairId, setViewingRunId]
  )

  const toggleSidebar = useCallback((): void => {
    setShellCollapsed((collapsed) => !collapsed)
  }, [])

  const openShortcuts = useCallback((): void => {
    setIsShortcutsOpen(true)
  }, [])

  const openPairSettings = useCallback((): void => {
    setIsPairSettingsOpen(true)
  }, [])

  const checkForUpdates = useCallback((): void => {
    void import('@tauri-apps/api/event').then(({ emit }) => emit('app:update:check'))
  }, [])

  const shortcuts: ShortcutDef[] = useMemo(
    () => [
      {
        key: 'n',
        modifiers: [cmdKey],
        handler: () => {
          if (selectedPair) {
            handleNewTask()
          } else {
            setIsCreatePairOpen(true)
          }
        },
        description: selectedPair ? t('shortcuts.newTask') : t('shortcuts.newPair')
      },
      {
        key: 'p',
        modifiers: [cmdKey],
        handler: () => {
          void handlePauseSelectedPair()
        },
        description: t('shortcuts.pausePair'),
        condition: () => selectedPair !== null && isPairActive(selectedPair.status)
      },
      {
        key: 'p',
        modifiers: [cmdKey, 'shift'],
        handler: () => {
          void handleResumeSelectedPair()
        },
        description: t('shortcuts.resumePair'),
        condition: () => selectedPair?.status === 'Paused'
      },
      {
        key: '?',
        modifiers: ['shift'],
        handler: () => {
          setIsShortcutsOpen((open) => !open)
        },
        description: t('shortcuts.showShortcuts')
      },
      {
        key: 'b',
        modifiers: [cmdKey, 'ctrl'],
        handler: () => {
          toggleSidebar()
        },
        description: t('shortcuts.toggleSidebar')
      },
      {
        key: 'k',
        modifiers: [cmdKey],
        handler: () => {
          if (isPaletteOpenRef.current) setIsPaletteOpen(false)
          else openPalette()
        },
        description: t('shortcuts.commandPalette')
      },
      {
        key: 'f',
        modifiers: [cmdKey],
        handler: () => {
          window.dispatchEvent(new CustomEvent('app:console:open-search'))
        },
        description: t('shortcuts.searchTranscript'),
        // Never while a modal dialog is open over the console: the console
        // listener would flush the search bar open and move focus out of the
        // dialog, leaving the reader typing behind a surface they can't see.
        condition: () =>
          canOpenConsoleSearch(selectedPair !== null, {
            createPairOpen: isCreatePairOpen,
            assignTaskOpen: isAssignTaskOpen,
            pairSettingsOpen: isPairSettingsOpen,
            paletteOpen: isPaletteOpen,
            shortcutsOpen: isShortcutsOpen,
            confirmDeletePairOpen: pendingDeletePair !== null,
            confirmClearSessionOpen: pendingClearPair !== null,
            updateModalOpen: updateModalVisible
          })
      },
      {
        key: '[',
        modifiers: [cmdKey],
        handler: () => {
          goHub()
        },
        description: t('shortcuts.goBack'),
        condition: () => selectedPair !== null
      },
      {
        key: '1',
        modifiers: [cmdKey],
        handler: () => {
          selectSection('pairs')
        },
        description: t('shortcuts.goPairs')
      },
      {
        key: '2',
        modifiers: [cmdKey],
        handler: () => {
          selectSection('activity')
        },
        description: t('shortcuts.goActivity')
      },
      {
        key: '3',
        modifiers: [cmdKey],
        handler: () => {
          selectSection('insights')
        },
        description: t('shortcuts.goInsights')
      }
    ],
    [
      selectedPair,
      t,
      handleNewTask,
      handlePauseSelectedPair,
      handleResumeSelectedPair,
      toggleSidebar,
      selectSection,
      goHub,
      openPalette,
      isCreatePairOpen,
      isAssignTaskOpen,
      isPairSettingsOpen,
      isPaletteOpen,
      isShortcutsOpen,
      pendingDeletePair,
      pendingClearPair,
      updateModalVisible
    ]
  )

  useShortcuts(shortcuts)

  // (selectPair / goHub are declared above with the other navigation callbacks.)

  // Native menu bar events (emitted from the Rust menu in lib.rs). The palette
  // actions below are the single source of truth — menus just re-dispatch here.
  useEffect(() => {
    const unlisteners: Array<() => void> = []
    let active = true
    const subs: Array<[string, () => void]> = [
      ['app:menu:new-pair', () => setIsCreatePairOpen(true)],
      [
        'app:menu:new-task',
        () => {
          if (selectedPair) handleNewTask()
          else setIsCreatePairOpen(true)
        }
      ],
      ['app:menu:pause', () => void handlePauseSelectedPair()],
      ['app:menu:resume', () => void handleResumeSelectedPair()],
      ['app:menu:back', () => goHub()],
      ['app:menu:go-pairs', () => selectSection('pairs')],
      ['app:menu:go-activity', () => selectSection('activity')],
      ['app:menu:go-insights', () => selectSection('insights')],
      ['app:menu:toggle-sidebar', () => toggleSidebar()],
      ['app:menu:toggle-theme', () => toggleTheme()],
      ['app:menu:shortcuts', () => openShortcuts()],
      ['app:menu:palette', () => openPalette()]
    ]
    for (const [event, handler] of subs) {
      void listen(event, handler).then((cleanup) => {
        if (active) unlisteners.push(cleanup)
        else cleanup()
      })
    }
    return () => {
      active = false
      for (const cleanup of unlisteners) cleanup()
    }
  }, [
    selectedPair,
    handleNewTask,
    handlePauseSelectedPair,
    handleResumeSelectedPair,
    toggleSidebar,
    toggleTheme,
    openShortcuts,
    openPalette,
    selectSection,
    selectPair,
    goHub
  ])

  const handleRequestClearSession = useCallback((): void => {
    if (!selectedPair) return
    setPendingClearPair(selectedPair)
  }, [selectedPair])

  const paletteContext: PaletteContext = useMemo(
    () => ({
      pairs,
      selectedPair,
      openCreatePair: () => setIsCreatePairOpen(true),
      openNewTask: handleNewTask,
      openPairSettings,
      pauseSelectedPair: () => void handlePauseSelectedPair(),
      resumeSelectedPair: () => void handleResumeSelectedPair(),
      clearSelectedSession: handleRequestClearSession,
      selectPair,
      selectSection,
      toggleSidebar,
      toggleTheme,
      openShortcuts,
      checkForUpdates
    }),
    [
      pairs,
      selectedPair,
      handleNewTask,
      openPairSettings,
      handlePauseSelectedPair,
      handleResumeSelectedPair,
      handleRequestClearSession,
      selectPair,
      selectSection,
      toggleSidebar,
      toggleTheme,
      openShortcuts,
      checkForUpdates
    ]
  )

  const paletteActions = useMemo(
    () => [...buildPaletteActions(paletteContext), ...buildPairJumpActions(pairs, selectPair)],
    [paletteContext, pairs, selectPair]
  )

  const palettePairName = useCallback(
    (actionId: string): string | null => {
      if (!actionId.startsWith('go.pair.')) return null
      const pair = pairs.find((p) => `go.pair.${p.id}` === actionId)
      return pair?.name ?? null
    },
    [pairs]
  )

  const handleRestoreTask = (spec: string, mentorModel: string, executorModel: string): void => {
    // Same guard as "New Task": starting a run while the pair is busy would
    // stop the run in progress.
    if (!selectedPair || isPairBusy(selectedPair.status)) return
    setRestoringSpec({ spec, mentorModel, executorModel })
    setIsAssignTaskOpen(true)
  }

  const handleDeletePair = (pair: Pair): void => {
    setPendingDeletePair(pair)
  }

  const confirmDeletePair = async (): Promise<void> => {
    if (!pendingDeletePair) return
    const pair = pendingDeletePair
    setPendingDeletePair(null)
    setDeletingPairId(pair.id)
    try {
      await deletePair(pair.id)
      if (selectedPairId === pair.id) {
        selectPair(null)
      }
    } catch (error) {
      // The backend aborts the delete (keeping the pair) when it can't preserve
      // the worktree's work — tell the user instead of failing silently.
      console.error('[App] Failed to delete pair:', error)
      displayToast(
        t('modals.deleteFailed', {
          error: extractErrorMessage(error, String(error))
        }),
        'error'
      )
    } finally {
      setDeletingPairId(null)
    }
  }

  const cancelDeletePair = (): void => {
    setPendingDeletePair(null)
  }

  const confirmClearSession = (): void => {
    if (!pendingClearPair) return
    setMessages(pendingClearPair.id, [])
    setPendingClearPair(null)
  }

  const cancelClearSession = (): void => {
    setPendingClearPair(null)
  }

  return (
    <div className="h-screen w-screen overflow-hidden bg-background font-sans text-foreground grain-overlay">
      <div className="flex h-full flex-col">
        <AppChrome
          selectedPair={selectedPair}
          readyModelCount={readyModelCount}
          totalModelCount={availableModels.length}
          modelsLoading={modelsLoading}
          theme={theme}
          onToggleTheme={toggleTheme}
          onNewTask={handleNewTask}
          onBack={selectedPair ? goHub : undefined}
          onClearSession={selectedPair ? handleRequestClearSession : undefined}
          onOpenSettings={selectedPair ? openPairSettings : undefined}
          onShowShortcuts={openShortcuts}
          onOpenPalette={openPalette}
        />

        <div className="flex min-h-0 flex-1 overflow-hidden">
          <AppShell
            section={shellSection}
            onSelectSection={selectSection}
            collapsed={shellCollapsed}
          >
            <ErrorBoundary>
              {selectedPair ? (
                <Dashboard
                  selectedPair={selectedPair}
                  onPauseSelectedPair={handlePauseSelectedPair}
                  onResumeSelectedPair={handleResumeSelectedPair}
                  onRestoreTask={handleRestoreTask}
                />
              ) : shellSection === 'activity' ? (
                <ActivityView pairs={pairs} onSelectPair={(pair) => selectPair(pair.id)} />
              ) : shellSection === 'insights' ? (
                <InsightsView />
              ) : (
                <PairsHub
                  pairs={pairs}
                  onSelectPair={(pair) => {
                    selectPair(pair.id)
                  }}
                  onDeletePair={(pair) => {
                    void handleDeletePair(pair)
                  }}
                  deletingPairId={deletingPairId}
                  onCreatePair={() => setIsCreatePairOpen(true)}
                  onPausePair={async (id: string) => {
                    try {
                      await pausePair(id)
                    } catch (e) {
                      console.error('[App] Failed to pause:', e)
                    }
                  }}
                  onResumePair={async (id: string) => {
                    try {
                      await resumePair(id)
                    } catch (e) {
                      console.error('[App] Failed to resume:', e)
                    }
                  }}
                />
              )}
            </ErrorBoundary>
          </AppShell>
        </div>
      </div>

      <BootSplash visible={!pairsLoaded || splashHoldVisible} minDurationMs={splashHoldDuration} />

      <ConfirmModal
        isOpen={pendingDeletePair !== null}
        title={t('modals.deleteTitle', { name: pendingDeletePair?.name ?? '' })}
        message={
          pendingDeletePair?.worktreePath
            ? `${t('modals.deleteDesc')} ${t('modals.deleteWorktreeNote')}`
            : t('modals.deleteDesc')
        }
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        onConfirm={confirmDeletePair}
        onCancel={cancelDeletePair}
      />

      <ConfirmModal
        isOpen={pendingClearPair !== null}
        title={t('chrome.clearSessionConfirmTitle', { name: pendingClearPair?.name ?? '' })}
        message={t('chrome.clearSessionConfirmMessage')}
        confirmLabel={t('chrome.clearSessionConfirmAction')}
        cancelLabel={t('common.cancel')}
        onConfirm={confirmClearSession}
        onCancel={cancelClearSession}
      />

      <Suspense fallback={null}>
        {shouldRenderCreatePairModal && (
          <CreatePairModal isOpen={isCreatePairOpen} onClose={() => setIsCreatePairOpen(false)} />
        )}
        {shouldRenderAssignTaskModal && (
          <AssignTaskModal
            key={
              selectedPair ? `assign-${selectedPair.id}-${String(isAssignTaskOpen)}` : 'assign-none'
            }
            pair={selectedPair}
            isOpen={isAssignTaskOpen}
            onClose={() => setIsAssignTaskOpen(false)}
          />
        )}
        {shouldRenderPairSettingsModal && (
          <PairSettingsModal
            key={
              selectedPair
                ? `settings-${selectedPair.id}-${String(isPairSettingsOpen)}`
                : 'settings-none'
            }
            pair={selectedPair}
            isOpen={isPairSettingsOpen}
            onClose={() => setIsPairSettingsOpen(false)}
          />
        )}
      </Suspense>

      <ShortcutsModal
        isOpen={isShortcutsOpen}
        onClose={() => setIsShortcutsOpen(false)}
        shortcuts={shortcuts}
      />

      <Suspense fallback={null}>
        {shouldRenderPalette && (
          <CommandPalette
            key={`palette-${paletteEpoch}`}
            isOpen={isPaletteOpen}
            onClose={() => setIsPaletteOpen(false)}
            actions={paletteActions}
            pairNameById={palettePairName}
          />
        )}
      </Suspense>

      <UpdateNotification />
    </div>
  )
}

export default App
