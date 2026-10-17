import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, ChevronUp, Search, Square, Trash2, X, XCircle } from 'lucide-react'
import { AnimatePresence } from 'framer-motion'
import { cn, extractErrorMessage, formatIterations } from '../lib/utils'
import { usePairStore, type Message, type Pair, type TurnCard } from '../store/usePairStore'
import { ScrollToBottomButton } from './ScrollToBottomButton'
import { ConfirmModal } from './ui/ConfirmModal'
import { MessageFilterBar } from './MessageFilterBar'
import { GlassButton } from './ui/GlassButton'
import { isPairActive } from '../lib/pairStatus'
import { isAgentExecuting } from '../lib/helpers'
import { MessageCard } from './MessageCard'
import { countSearchHighlights } from '../lib/searchableCard'
import { TurnCardView } from './TurnCardView'
import { PlanReviewBar } from './PlanReviewBar'
import { SystemBanner } from './SystemBanner'
import { TerminalBlock } from './terminal/TerminalBlock'
import { collapseWithDropCounts } from '../lib/consoleMessages'
import { nextMatchIndex } from '../lib/consoleSearch'
import { FileMention } from './FileMention'
import { SkillMention } from './SkillMention'
import { type FileContexts } from '../lib/fileMentions'
import { composeFinalSpec, type SkillContexts } from '../lib/skillMentions'
import { isImeKeyEvent, isSubmitEnter } from './keyboard'
import { useCompositionTracker } from './useCompositionTracker'
import {
  isNearBottom,
  resolveViewingRun,
  selectVisibleTurnCard,
  splitComposition
} from './consoleView'

interface PairConsoleProps {
  pair: Pair
  className?: string
}

interface CompositionState {
  text: string
  start: number | null
}

const NO_COMPOSITION: CompositionState = { text: '', start: null }

function PairConsole({ pair, className }: PairConsoleProps): React.ReactNode {
  const { t } = useTranslation()
  const killProcess = usePairStore((s) => s.killProcess)
  const setMessages = usePairStore((s) => s.setMessages)
  const assignTask = usePairStore((s) => s.assignTask)
  const viewingRunId = usePairStore((s) => s.viewingRunId)
  const setViewingRunId = usePairStore((s) => s.setViewingRunId)
  const scrollRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  // Whether the console should follow new output. Updated from the user's own
  // scrolling so a reader scrolled up into the history is never yanked down.
  const stickToBottomRef = useRef(true)
  const composition = useCompositionTracker()
  const [messageFilter, setMessageFilter] = useState<'all' | 'mentor' | 'executor'>('all')
  const [taskInput, setTaskInput] = useState('')
  const [composingState, setComposingState] = useState<CompositionState>(NO_COMPOSITION)
  const [isSubmittingTask, setIsSubmittingTask] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [isStoppingTurn, setIsStoppingTurn] = useState(false)
  // Scoped to the card it was raised for, so a stale failure never shows under a later turn.
  const [stopError, setStopError] = useState<{ cardId: string; message: string } | null>(null)
  const [fileContexts, setFileContexts] = useState<FileContexts>(new Map())
  const [skillContexts, setSkillContexts] = useState<SkillContexts>(new Map())
  const [confirmClear, setConfirmClear] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [matchOrdinal, setMatchOrdinal] = useState(-1)
  // Bumped on every cycle call, even when the ordinal doesn't change (a single
  // match), so the scroll effect below re-runs on every press.
  const [matchCycle, setMatchCycle] = useState(0)
  const searchInputRef = useRef<HTMLInputElement>(null)

  const handleFileSelect = useCallback((path: string, content: string): void => {
    setFileContexts((prev) => {
      const next = new Map(prev)
      next.set(path, content)
      return next
    })
  }, [])

  const handleSkillSelect = useCallback((name: string, description: string, body: string): void => {
    setSkillContexts((prev) => {
      const next = new Map(prev)
      next.set(name, { description, body })
      return next
    })
  }, [])

  // An id that isn't in this pair's history (e.g. left over from another pair)
  // means "live" — never lock this pair's console into a phantom archive view.
  const viewingRun = resolveViewingRun(pair.runHistory, viewingRunId)
  const isViewingArchived = viewingRun !== null

  const roleFilteredMessages = useMemo(() => {
    const messages = viewingRun ? viewingRun.messages : pair.messages
    if (messageFilter === 'all') return messages
    return messages.filter((msg) => msg.from === 'human' || msg.from === messageFilter)
  }, [pair.messages, viewingRun, messageFilter])

  const searching = searchOpen && searchQuery.trim().length > 0
  // Dedup before the search filter, so which drafts collapse never depends on
  // the query: search keeps the same finals as the unsearched view, a draft
  // hidden without a query stays hidden with one, and the final's drop marker
  // survives.
  const { deduplicatedConsoleMessages, dropCountByMessageId } = useMemo(() => {
    const { kept, droppedBeforeId } = collapseWithDropCounts<Message>(roleFilteredMessages)
    return { deduplicatedConsoleMessages: kept, dropCountByMessageId: droppedBeforeId }
  }, [roleFilteredMessages])

  // Filter on the same rendered-text count that drives the badge and Enter
  // cycling, so a message whose only raw-source hit is a link URL or another
  // non-rendered syntax never stays listed without a highlight.
  const consoleMessages = useMemo(
    () =>
      searching
        ? deduplicatedConsoleMessages.filter((m) => countSearchHighlights(m, searchQuery) > 0)
        : deduplicatedConsoleMessages,
    [deduplicatedConsoleMessages, searching, searchQuery]
  )

  // Count and cycle only over matches that render as highlighted, anchored
  // cards — the search-filtered list, so collapsed-away messages never
  // inflate the count and Enter never lands on a message with no
  // `console-msg-<id>` element.
  const searchMatches = useMemo(() => {
    if (!searching) return [] as Array<{ id: string; count: number }>
    return consoleMessages.map((m) => ({ id: m.id, count: countSearchHighlights(m, searchQuery) }))
  }, [consoleMessages, searching, searchQuery])

  const matchCount = useMemo(() => searchMatches.reduce((n, x) => n + x.count, 0), [searchMatches])

  const messageCounts = useMemo(() => {
    const allMessages = viewingRun ? viewingRun.messages : pair.messages
    return {
      mentor: allMessages.filter((msg) => msg.from === 'mentor').length,
      executor: allMessages.filter((msg) => msg.from === 'executor').length,
      all: allMessages.length
    }
  }, [pair.messages, viewingRun])

  const matchingMessageIds = useMemo(() => searchMatches.map((x) => x.id), [searchMatches])
  const activeMessageId =
    matchOrdinal >= 0 && matchOrdinal < matchingMessageIds.length
      ? matchingMessageIds[matchOrdinal]
      : null

  const cycleMatch = useCallback(
    (dir: 1 | -1): void => {
      setMatchOrdinal(nextMatchIndex(matchOrdinal, matchingMessageIds.length, dir))
      setMatchCycle((cycle) => cycle + 1)
    },
    [matchOrdinal, matchingMessageIds]
  )

  // Scroll from an effect, after React commits the new active card: the card
  // lifts its 280px clip and the previously active card collapses in that
  // same commit, so the smooth scroll must pick its destination from the
  // final layout, not the pre-commit one. Aim at the first highlight — the
  // card's top edge can leave a match deep in a long card off-screen.
  useEffect(() => {
    if (!activeMessageId) return
    const card = document.getElementById(`console-msg-${activeMessageId}`)
    if (!card) return
    const target = card.querySelector('mark') ?? card
    target.scrollIntoView({ block: 'center', behavior: 'smooth' })
  }, [activeMessageId, matchCycle])

  const closeSearch = useCallback((): void => {
    setSearchOpen(false)
    setSearchQuery('')
    setMatchOrdinal(-1)
  }, [])

  // Cmd/Ctrl+F opens transcript search while this console is mounted.
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f') {
        e.preventDefault()
        setSearchOpen(true)
        requestAnimationFrame(() => searchInputRef.current?.focus())
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  useEffect(() => {
    setMatchOrdinal(-1)
  }, [searchQuery, pair.id, viewingRunId])

  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    const handleScroll = (): void => {
      stickToBottomRef.current = isNearBottom(el)
    }
    el.addEventListener('scroll', handleScroll, { passive: true })
    return () => el.removeEventListener('scroll', handleScroll)
  }, [])

  // Opening the pair (or returning from an archived run) lands on the newest output.
  useEffect(() => {
    if (isViewingArchived) return
    stickToBottomRef.current = true
    const el = scrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [isViewingArchived])

  // Follow new output only while the reader is already at the bottom, and never
  // while reading an archived run.
  useEffect(() => {
    if (isViewingArchived || !stickToBottomRef.current) return
    const el = scrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [isViewingArchived, pair.status, pair.messages.length, pair.currentTurnCard?.updatedAt])

  // The live turn card belongs to the live run only; compare against the
  // unfiltered live transcript so a role filter can't hide it.
  const visibleCurrentTurnCard = isViewingArchived
    ? null
    : selectVisibleTurnCard(pair.messages, pair.currentTurnCard)

  const renderedChat = useMemo(() => {
    const nodes: React.ReactNode[] = []
    let lastIteration = -1
    for (const msg of consoleMessages) {
      const iter = msg.iteration
      if (iter > 0 && iter !== lastIteration) {
        nodes.push(
          <SystemBanner
            key={`iter-${iter}-${msg.id}`}
            variant="iteration"
            iteration={iter}
            maxIterations={pair.maxIterations}
          />
        )
        lastIteration = iter
      }

      const isHuman = msg.from === 'human'
      // Mission spec (the launching task at iteration 0) reads as a brief — let it span the full column.
      const isMission = isHuman && !(msg.type === 'feedback' && msg.iteration > 0)

      const maxWidthClass = isMission ? 'w-full' : 'max-w-[80%]'
      const alignClass = isMission ? 'justify-stretch' : 'justify-start'

      const droppedBefore = dropCountByMessageId.get(msg.id) ?? 0
      if (droppedBefore > 0) {
        nodes.push(
          <div key={`drops-${msg.id}`} className={cn('flex', alignClass)}>
            <div className={cn(maxWidthClass, 'pl-[3ch]')}>
              <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground-faint">
                {t('console.earlierMessagesDropped', { count: droppedBefore })}
              </span>
            </div>
          </div>
        )
      }

      nodes.push(
        <div key={msg.id} className={cn('flex', alignClass)}>
          <div className={cn(maxWidthClass)}>
            <MessageCard
              msg={msg}
              searchQuery={searching ? searchQuery : undefined}
              isActiveMatch={searching && msg.id === activeMessageId}
            />
          </div>
        </div>
      )
    }
    return nodes
  }, [
    consoleMessages,
    dropCountByMessageId,
    pair.maxIterations,
    t,
    searching,
    searchQuery,
    activeMessageId
  ])

  const handleClearMessages = (): void => {
    // Same confirm as the toolbar/palette path (App.tsx) — never wipe a
    // transcript from a single misclick.
    setConfirmClear(true)
  }

  const submitTask = async (): Promise<void> => {
    const spec = taskInput.trim()
    if (!spec || isSubmittingTask) return
    // Guard: if pair is running, refuse silently. The UI already disables the textarea,
    // but keep this as a belt-and-suspenders fallback against stale state.
    if (isPairActive(pair.status)) return

    setIsSubmittingTask(true)
    setSubmitError(null)
    try {
      const finalSpec = composeFinalSpec(spec, fileContexts, skillContexts, pair.executorProvider)
      await assignTask(pair.id, finalSpec)
      setTaskInput('')
      setFileContexts(new Map())
      setSkillContexts(new Map())
      stickToBottomRef.current = true
      requestAnimationFrame(() => {
        const el = scrollRef.current
        if (el) el.scrollTop = el.scrollHeight
        inputRef.current?.focus()
      })
    } catch (error) {
      // The store's global error is only rendered inside modals — surface the
      // failure right next to the input so a failed submit is never silent.
      setSubmitError(extractErrorMessage(error, t('console.submitFailed')))
    } finally {
      setIsSubmittingTask(false)
    }
  }

  const handleTaskSubmit = (e: React.FormEvent): void => {
    e.preventDefault()
    void submitTask()
  }

  const handleInputKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>): void => {
    // Enter that confirms an IME candidate must never submit the task.
    if (isSubmitEnter(e, composition.isComposing())) {
      e.preventDefault()
      void submitTask()
    }
  }

  const handleStopTurn = async (card: TurnCard): Promise<void> => {
    if (isStoppingTurn) return
    setIsStoppingTurn(true)
    setStopError(null)
    try {
      await killProcess(pair.id, card.role)
    } catch (error) {
      setStopError({
        cardId: card.id,
        message: extractErrorMessage(error, t('console.stopFailed'))
      })
    } finally {
      setIsStoppingTurn(false)
    }
  }

  const composingText = composingState.text
  const inputSegments = splitComposition(taskInput, composingText, composingState.start)
  const hasText = taskInput.length > 0 || composingText.length > 0

  const mentorIsExecuting = isAgentExecuting(pair.mentorActivity.phase)
  const executorIsExecuting = isAgentExecuting(pair.executorActivity.phase)
  const isRunning = isPairActive(pair.status) || mentorIsExecuting || executorIsExecuting
  // Input is "locked" while the pair runs — submitting a new task would silently
  // archive the in-progress run, which is almost never what the user wants.
  const inputLocked = isRunning

  const taskInputRow = !isViewingArchived ? (
    <form
      onSubmit={handleTaskSubmit}
      className={cn('mt-2', inputLocked ? 'cursor-not-allowed opacity-60' : 'cursor-text')}
      onClick={() => {
        if (!inputLocked) inputRef.current?.focus()
      }}
      data-testid="pair-task-input"
      aria-disabled={inputLocked}
    >
      <div className="flex items-baseline gap-2 font-mono text-[12px] leading-relaxed">
        <span
          aria-hidden
          className={cn(
            'select-none',
            inputLocked ? 'text-muted-foreground-faint' : 'state-running'
          )}
        >
          {'>'}
        </span>
        <div className="relative flex-1 min-w-0">
          <div
            aria-hidden
            className={cn(
              'whitespace-pre-wrap break-words font-mono text-[12px]',
              inputLocked ? 'text-muted-foreground-faint' : 'text-foreground'
            )}
          >
            {hasText ? (
              <>
                <span>{inputSegments.before}</span>
                {inputSegments.composing && (
                  <span className="text-muted-foreground underline decoration-dotted">
                    {inputSegments.composing}
                  </span>
                )}
                {inputSegments.after && <span>{inputSegments.after}</span>}
                <span
                  className={cn(
                    'select-none ml-[1px]',
                    inputLocked ? 'text-muted-foreground-faint' : 'state-running',
                    isSubmittingTask || inputLocked ? '' : 'tty-blink'
                  )}
                >
                  ▍
                </span>
              </>
            ) : (
              <>
                <span
                  className={cn(
                    'select-none mr-[2px]',
                    inputLocked ? 'text-muted-foreground-faint' : 'state-running',
                    isSubmittingTask || inputLocked ? '' : 'tty-blink'
                  )}
                >
                  ▍
                </span>
                <span className="text-muted-foreground-faint">
                  {inputLocked
                    ? t('console.inputDisabledRunningHint')
                    : t('console.taskInputPlaceholder')}
                </span>
              </>
            )}
          </div>
          <textarea
            ref={inputRef}
            rows={1}
            value={taskInput}
            onChange={(e) => {
              setTaskInput(e.target.value)
              if (submitError) setSubmitError(null)
            }}
            onKeyDown={handleInputKeyDown}
            onCompositionStart={(e) => {
              composition.onCompositionStart()
              const start = e.currentTarget.selectionStart
              setComposingState({ text: e.data || '', start })
            }}
            onCompositionUpdate={(e) => {
              const data = e.data || ''
              setComposingState((prev) => ({ ...prev, text: data }))
            }}
            onCompositionEnd={() => {
              composition.onCompositionEnd()
              setComposingState(NO_COMPOSITION)
            }}
            aria-label={
              inputLocked
                ? t('console.inputDisabledRunningHint')
                : t('console.taskInputPlaceholder')
            }
            autoComplete="off"
            spellCheck={false}
            disabled={isSubmittingTask || inputLocked}
            className={cn(
              'absolute inset-0 w-full h-full resize-none overflow-hidden bg-transparent border-0 outline-none p-0 font-mono text-[12px] leading-relaxed text-transparent',
              inputLocked && 'pointer-events-none'
            )}
            style={{ caretColor: 'transparent' }}
          />
          {pair.directory && !inputLocked && (
            <FileMention
              textareaRef={inputRef}
              onChange={setTaskInput}
              directory={pair.directory}
              pairId={pair.id}
              onFileSelect={handleFileSelect}
            />
          )}
          {!inputLocked && (
            <SkillMention
              textareaRef={inputRef}
              onChange={setTaskInput}
              projectDir={pair.directory}
              onSkillSelect={handleSkillSelect}
            />
          )}
        </div>
      </div>
      {(fileContexts.size > 0 || skillContexts.size > 0) && (
        <div className="mt-1.5 flex flex-wrap gap-1 pl-[2ch]">
          {Array.from(skillContexts.keys())
            .filter((name) => taskInput.includes(`/${name}`))
            .map((name) => (
              <span
                key={`skill-${name}`}
                className="inline-flex items-baseline gap-1 rounded-sm border border-border bg-foreground/[0.04] px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground"
                title={t('console.attachedSkill', { name })}
              >
                <span className="role-mentor">/</span>
                <span className="truncate max-w-[28ch]">{name}</span>
              </span>
            ))}
          {Array.from(fileContexts.keys())
            .filter((path) => taskInput.includes(`@${path}`))
            .map((path) => (
              <span
                key={`file-${path}`}
                className="inline-flex items-baseline gap-1 rounded-sm border border-border bg-foreground/[0.04] px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground"
                title={t('console.attachedFile', { path })}
              >
                <span className="role-mentor">@</span>
                <span className="truncate max-w-[28ch]">{path}</span>
              </span>
            ))}
        </div>
      )}
      {submitError && (
        <div
          role="alert"
          data-testid="pair-task-submit-error"
          className="mt-1.5 flex items-baseline gap-1.5 pl-[2ch] font-mono text-[11px] state-error [overflow-wrap:anywhere]"
        >
          <span aria-hidden className="select-none">
            ✗
          </span>
          <span>{submitError}</span>
        </div>
      )}
      {pair.handoffError && (
        <div
          role="alert"
          data-testid="pair-handoff-error"
          className="mt-1.5 flex items-baseline gap-1.5 pl-[2ch] font-mono text-[11px] state-error [overflow-wrap:anywhere]"
        >
          <span aria-hidden className="select-none">
            ✗
          </span>
          <span>{pair.handoffError}</span>
        </div>
      )}
    </form>
  ) : null

  if (!pair || !pair.id || !pair.name) {
    console.error('[PairConsole] Invalid pair data:', pair)
    return (
      <div className="flex h-full items-center justify-center">
        <div className="text-center font-mono">
          <p className="state-error text-base font-bold uppercase">! {t('pair.invalidData')}</p>
          <p className="mt-2 text-xs text-muted-foreground">{t('pair.invalidDataDesc')}</p>
        </div>
      </div>
    )
  }

  return (
    <div className={cn('flex h-full flex-col bg-background', className)}>
      {/* Header bar */}
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border bg-background px-4 font-mono text-[11px] text-muted-foreground">
        <span aria-hidden className="select-none text-foreground/75">
          {'>_'}
        </span>
        <span className="uppercase tracking-[0.14em] font-bold text-foreground/90">
          {isViewingArchived ? t('history.title') : t('pair.sessionConsole')}
        </span>
        {isViewingArchived && (
          <>
            <span className="text-muted-foreground-faint">· {t('console.viewingArchived')}</span>
            <button
              type="button"
              onClick={() => setViewingRunId(null)}
              data-testid="console-back-to-live-btn"
              className="rounded-sm border border-border px-1.5 py-px text-[10px] uppercase tracking-[0.14em] text-foreground/85 transition-colors hover:border-foreground/40 hover:bg-foreground/[0.06] cursor-pointer"
            >
              ← {t('history.backToCurrent')}
            </button>
          </>
        )}
        <span className="text-muted-foreground-faint">·</span>
        <span className="tabular-nums">
          iter {formatIterations(pair.iterations, pair.maxIterations)}
        </span>
        <div className="ml-auto flex items-center gap-3">
          {!isViewingArchived && (
            <MessageFilterBar
              activeFilter={messageFilter}
              onFilterChange={setMessageFilter}
              counts={messageCounts}
            />
          )}
          <div className="flex items-center gap-1.5">
            <span
              aria-hidden
              className={cn(
                'inline-block tabular-nums select-none',
                isRunning ? 'state-running tty-blink' : 'text-muted-foreground-faint'
              )}
            >
              ●
            </span>
            <span className="text-[10px] uppercase tracking-[0.14em]">
              {isRunning ? t('pair.systemOnline') : t('pair.systemIdle')}
            </span>
          </div>
          {!isViewingArchived && pair.messages.length > 0 && !isRunning && (
            <button
              onClick={handleClearMessages}
              title={t('console.clearHistory')}
              data-testid="console-clear-btn"
              className="p-0.5 text-muted-foreground-faint hover:text-state-error transition-colors cursor-pointer"
            >
              <Trash2 size={12} />
            </button>
          )}
          <button
            type="button"
            onClick={() => {
              setSearchOpen(true)
              requestAnimationFrame(() => searchInputRef.current?.focus())
            }}
            title={t('console.searchOpen')}
            aria-label={t('console.searchOpen')}
            data-testid="console-search-toggle"
            className="p-0.5 text-muted-foreground-faint hover:text-foreground transition-colors cursor-pointer"
          >
            <Search size={12} />
          </button>
        </div>
      </div>

      {searchOpen && (
        <div className="flex h-8 shrink-0 items-center gap-2 border-b border-border bg-background px-4 font-mono text-[11px] text-muted-foreground">
          <Search size={11} aria-hidden className="shrink-0 text-muted-foreground-faint" />
          <input
            ref={searchInputRef}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyDown={(e) => {
              // The Enter that confirms an IME candidate and the Escape that
              // cancels one belong to the composition, not the search.
              if (isImeKeyEvent(e)) return
              if (e.key === 'Escape') {
                e.preventDefault()
                closeSearch()
              } else if (e.key === 'Enter') {
                e.preventDefault()
                cycleMatch(e.shiftKey ? -1 : 1)
              }
            }}
            placeholder={t('console.searchPlaceholder')}
            aria-label={t('console.searchPlaceholder')}
            data-testid="console-search-input"
            className="min-w-0 flex-1 bg-transparent text-foreground outline-none placeholder:text-muted-foreground-faint"
          />
          {searchQuery.trim() && (
            <span data-testid="console-search-count" className="shrink-0 tabular-nums">
              {matchCount > 0
                ? t('console.searchMatches', { count: matchCount })
                : t('console.searchNoMatches')}
            </span>
          )}
          <button
            type="button"
            onClick={() => cycleMatch(-1)}
            disabled={matchingMessageIds.length === 0}
            aria-label={t('console.searchPrev')}
            title={t('console.searchPrev')}
            data-testid="console-search-prev"
            className="p-0.5 text-muted-foreground hover:text-foreground disabled:opacity-40 transition-colors cursor-pointer"
          >
            <ChevronUp size={12} />
          </button>
          <button
            type="button"
            onClick={() => cycleMatch(1)}
            disabled={matchingMessageIds.length === 0}
            aria-label={t('console.searchNext')}
            title={t('console.searchNext')}
            data-testid="console-search-next"
            className="p-0.5 text-muted-foreground hover:text-foreground disabled:opacity-40 transition-colors cursor-pointer"
          >
            <ChevronDown size={12} />
          </button>
          <button
            type="button"
            onClick={closeSearch}
            aria-label={t('console.searchClose')}
            title={t('console.searchClose')}
            data-testid="console-search-close"
            className="p-0.5 text-muted-foreground hover:text-foreground transition-colors cursor-pointer"
          >
            <X size={12} />
          </button>
        </div>
      )}

      {/* Message area */}
      <div className="relative flex min-h-0 flex-1 flex-col">
        <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto scrollbar-thin">
          <div className="flex w-full flex-col gap-2 px-6 py-6">
            {deduplicatedConsoleMessages.length === 0 && !visibleCurrentTurnCard ? (
              <>
                <div className="flex flex-col items-start gap-1 pt-12 font-mono text-[12px] text-muted-foreground">
                  <span className="uppercase tracking-[0.14em] text-foreground/80">
                    {t('pair.freshSession')}
                  </span>
                  <span className="text-[11px] text-muted-foreground-faint">
                    {t('pair.awaitingFirst')}
                  </span>
                </div>
                {taskInputRow}
              </>
            ) : (
              <>
                {renderedChat}
                {((!isViewingArchived && isPairActive(pair.status)) || visibleCurrentTurnCard) && (
                  <AnimatePresence mode="popLayout">
                    {visibleCurrentTurnCard ? (
                      <div key={visibleCurrentTurnCard.id} className="space-y-1">
                        <TurnCardView card={visibleCurrentTurnCard} />
                        {isRunning && (
                          <div className="pl-[3ch] flex flex-wrap items-center gap-2">
                            <GlassButton
                              variant={
                                visibleCurrentTurnCard.activity.phase === 'stalled'
                                  ? 'destructive'
                                  : 'secondary'
                              }
                              size="sm"
                              className="gap-1.5"
                              onClick={() => {
                                void handleStopTurn(visibleCurrentTurnCard)
                              }}
                              disabled={isStoppingTurn}
                              title={t('console.stopTurn')}
                              data-testid="console-stop-turn-btn"
                            >
                              {visibleCurrentTurnCard.activity.phase === 'stalled' ? (
                                <>
                                  <XCircle size={11} />
                                  {t('pair.killProcess')}
                                </>
                              ) : (
                                <>
                                  <Square size={10} />
                                  {t('console.stopTurn')}
                                </>
                              )}
                            </GlassButton>
                            {stopError?.cardId === visibleCurrentTurnCard.id && (
                              <span
                                role="alert"
                                className="font-mono text-[11px] state-error [overflow-wrap:anywhere]"
                              >
                                ✗ {stopError.message}
                              </span>
                            )}
                          </div>
                        )}
                      </div>
                    ) : (
                      <TerminalBlock
                        key="active-placeholder"
                        role={pair.turn}
                        state="done"
                        label={(pair.turn === 'mentor'
                          ? t('common.mentor')
                          : t('common.executor')
                        ).toUpperCase()}
                        accentBorder
                        meta={
                          <span className="text-muted-foreground normal-case tracking-normal">
                            {pair.turn === 'mentor'
                              ? pair.mentorActivity.label
                              : pair.executorActivity.label}
                          </span>
                        }
                      >
                        <span className="flex items-baseline gap-1.5 text-muted-foreground">
                          <span
                            aria-hidden
                            className="inline-flex h-[1em] w-[1.4ch] items-center justify-center state-running"
                          >
                            <span className="tty-spin">✻</span>
                          </span>
                          {t('pair.thinking')}
                        </span>
                      </TerminalBlock>
                    )}
                  </AnimatePresence>
                )}
                {pair.status === 'Awaiting Human Review' &&
                  (isViewingArchived ? (
                    // While viewing an archived run, don't render the approve/reject
                    // controls (they act on the live pair, not the run on screen);
                    // surface a clear one-click return to the pending plan instead.
                    <div className="pl-[3ch]">
                      <button
                        type="button"
                        onClick={() => setViewingRunId(null)}
                        className="flex w-full items-center gap-1.5 rounded-sm border border-state-running/40 bg-state-running/[0.06] p-3 text-left font-mono text-[11px] state-running transition-colors hover:bg-state-running/[0.1]"
                      >
                        <span aria-hidden>▸</span>
                        <span className="font-bold uppercase tracking-[0.16em]">
                          {t('planReview.reviewPending')}
                        </span>
                        <span className="ml-auto text-muted-foreground">
                          {t('planReview.returnToReview')} →
                        </span>
                      </button>
                    </div>
                  ) : (
                    <PlanReviewBar pair={pair} />
                  ))}
                {taskInputRow}
              </>
            )}
          </div>
        </div>
        <ScrollToBottomButton
          scrollRef={scrollRef}
          dependency={`${consoleMessages.length}-${messageFilter}-${viewingRun?.id ?? 'live'}`}
        />
      </div>
      <ConfirmModal
        isOpen={confirmClear}
        title={t('chrome.clearSessionConfirmTitle', { name: pair.name })}
        message={t('chrome.clearSessionConfirmMessage')}
        confirmLabel={t('chrome.clearSessionConfirmAction')}
        cancelLabel={t('common.cancel')}
        onConfirm={() => {
          setMessages(pair.id, [])
          setConfirmClear(false)
        }}
        onCancel={() => setConfirmClear(false)}
      />
    </div>
  )
}

export default PairConsole
