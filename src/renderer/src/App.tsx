import React, { lazy, Suspense, useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { useTranslation } from 'react-i18next'
import { usePairStore, type Pair } from './store/usePairStore'
import { useThemeStore } from './store/useThemeStore'
import { useUpdateStore } from './store/useUpdateStore'
import './i18n'
import { AppChrome } from './components/AppChrome'
import { BootSplash } from './components/BootSplash'
import { ConfirmModal } from './components/ui/ConfirmModal'
import { UpdateNotification } from './components/UpdateNotification'
import { isSelectableForPairExecution } from './lib/modelPreferences'
import { ErrorBoundary } from './components/ErrorBoundary'
import { isPairActive } from './lib/pairStatus'
import { Dashboard } from './components/Dashboard'
import { preloadSounds } from './lib/sound'
import { useShortcuts } from './hooks/useShortcuts'

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
  const [isCreatePairOpen, setIsCreatePairOpen] = useState(false)
  const [isAssignTaskOpen, setIsAssignTaskOpen] = useState(false)
  const [isPairSettingsOpen, setIsPairSettingsOpen] = useState(false)
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
  const setMessages = usePairStore((state) => state.setMessages)

  const theme = useThemeStore((state) => state.theme)
  const toggleTheme = useThemeStore((state) => state.toggleTheme)

  const setPhase = useUpdateStore((state) => state.setPhase)
  const setVersion = useUpdateStore((state) => state.setVersion)
  const setMessage = useUpdateStore((state) => state.setMessage)
  const setReleaseBody = useUpdateStore((state) => state.setReleaseBody)
  const setUpdate = useUpdateStore((state) => state.setUpdate)
  const setShowModal = useUpdateStore((state) => state.setShowModal)
  const displayToast = useUpdateStore((state) => state.displayToast)
  const updateRef = useRef<Update | null>(null)

  useEffect(() => {
    let unlisten: (() => void) | undefined

    const performUpdateCheck = async (): Promise<void> => {
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
      unlisten = cleanup
    })

    const cancelQueuedUpdateCheck = import.meta.env.PROD
      ? queueStartupUpdateCheck(() => {
          void performUpdateCheck()
        })
      : undefined

    return () => {
      cancelQueuedUpdateCheck?.()
      unlisten?.()
      if (updateRef.current) {
        void updateRef.current.close().catch(() => {})
      }
    }
  }, [setPhase, setVersion, setMessage, setReleaseBody, setUpdate, setShowModal, displayToast])

  useEffect(() => {
    preloadSounds()
    const init = async (): Promise<void> => {
      initMessageListener()
      void loadAvailableModels()
      await loadAllPairs()
      setPairsLoaded(true)
    }
    void init()
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
    const handleBeforeUnload = (): void => {
      void flushSnapshots()
    }

    window.addEventListener('beforeunload', handleBeforeUnload)
    return () => window.removeEventListener('beforeunload', handleBeforeUnload)
  }, [flushSnapshots])

  const selectedPair = pairs.find((p) => p.id === selectedPairId) ?? null
  const readyModelCount = availableModels.filter((model) =>
    isSelectableForPairExecution(model)
  ).length
  const shouldRenderCreatePairModal = hasCreatePairModalLoaded || isCreatePairOpen
  const shouldRenderAssignTaskModal = hasAssignTaskModalLoaded || isAssignTaskOpen
  const shouldRenderPairSettingsModal = hasPairSettingsModalLoaded || isPairSettingsOpen

  // 如果选中了一个不存在的 pair，重置选择
  useEffect(() => {
    if (selectedPairId && !selectedPair) {
      setSelectedPairId(null)
    }
  }, [selectedPairId, selectedPair])

  const handlePauseSelectedPair = async (): Promise<void> => {
    if (!selectedPair || !isPairActive(selectedPair.status)) return

    try {
      await pausePair(selectedPair.id)
    } catch (error) {
      console.error('[App] Failed to pause pair:', error)
    }
  }

  const handleResumeSelectedPair = async (): Promise<void> => {
    if (!selectedPair || selectedPair.status !== 'Paused') return

    try {
      await resumePair(selectedPair.id)
    } catch (error) {
      console.error('[App] Failed to resume pair:', error)
    }
  }

  const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/.test(navigator.platform)
  const cmdKey = isMac ? 'meta' : 'ctrl'

  useShortcuts([
    {
      key: 'p',
      modifiers: [cmdKey],
      handler: () => {
        void handlePauseSelectedPair()
      },
      description: 'Pause Pair',
      condition: () => selectedPair !== null && isPairActive(selectedPair.status)
    },
    {
      key: 'p',
      modifiers: [cmdKey, 'shift'],
      handler: () => {
        void handleResumeSelectedPair()
      },
      description: 'Resume Pair',
      condition: () => selectedPair?.status === 'Paused'
    },
    {
      key: 'n',
      modifiers: [cmdKey],
      handler: () => setIsCreatePairOpen(true),
      description: 'New Pair'
    }
  ])

  const handleRestoreTask = (spec: string, mentorModel: string, executorModel: string): void => {
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
        setSelectedPairId(null)
      }
    } catch (error) {
      console.error('[App] Failed to delete pair:', error)
    } finally {
      setDeletingPairId(null)
    }
  }

  const cancelDeletePair = (): void => {
    setPendingDeletePair(null)
  }

  const handleRequestClearSession = (): void => {
    if (!selectedPair) return
    setPendingClearPair(selectedPair)
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
    <div className="h-screen w-screen overflow-hidden bg-background font-sans text-foreground selection:bg-primary selection:text-primary-foreground grain-overlay">
      <div className="flex h-full flex-col">
        <AppChrome
          selectedPair={selectedPair}
          readyModelCount={readyModelCount}
          totalModelCount={availableModels.length}
          modelsLoading={modelsLoading}
          theme={theme}
          onToggleTheme={toggleTheme}
          onNewPair={() => setIsCreatePairOpen(true)}
          onBack={selectedPair ? () => setSelectedPairId(null) : undefined}
          onClearSession={selectedPair ? handleRequestClearSession : undefined}
          onOpenSettings={selectedPair ? () => setIsPairSettingsOpen(true) : undefined}
        />

        <div className="flex-1 overflow-hidden">
          <ErrorBoundary>
            <Dashboard
              selectedPair={selectedPair}
              selectedPairId={selectedPairId}
              onSelectPair={(pair) => {
                setSelectedPairId(pair.id)
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
              onPauseSelectedPair={handlePauseSelectedPair}
              onResumeSelectedPair={handleResumeSelectedPair}
              onRestoreTask={handleRestoreTask}
            />
          </ErrorBoundary>
        </div>
      </div>

      <BootSplash visible={!pairsLoaded || splashHoldVisible} minDurationMs={splashHoldDuration} />

      <ConfirmModal
        isOpen={pendingDeletePair !== null}
        title={`Delete "${pendingDeletePair?.name}"?`}
        message="This will permanently remove the pair, its snapshot, and its recoverable session."
        confirmLabel="Delete"
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

      <UpdateNotification />
    </div>
  )
}

export default App
