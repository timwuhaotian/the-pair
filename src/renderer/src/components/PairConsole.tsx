import React, { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from 'react'
import { flushSync } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { ChevronDown, ChevronUp, Search, Square, Trash2, X, XCircle } from 'lucide-react'
import { AnimatePresence } from 'framer-motion'
import { cn, extractErrorMessage, formatIterations } from '../lib/utils'
import { usePairStore, type Message, type Pair, type TurnCard } from '../store/usePairStore'
import { ScrollToBottomButton } from './ScrollToBottomButton'
import { ConfirmModal } from './ui/ConfirmModal'
import { MessageFilterBar } from './MessageFilterBar'
import { GlassButton } from './ui/GlassButton'
import { IconButton } from './ui/IconButton'
import { Chip } from './ui/Chip'
import { BackToCurrentButton } from './TaskHistoryPanel'
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
  // The drawn caret only blinks while the (invisible) textarea has focus.
  const [inputFocused, setInputFocused] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  // The transcript list lags one keystroke behind the input on purpose.
  // Filtering, match counting, and re-highlighting every matching card through
  // react-markdown is expensive at transcript scale, and running it
  // synchronously per character makes the input itself janky. The deferred
  // value keeps the urgent input render cheap and lets the list catch up.
  const listQuery = useDeferredValue(searchQuery)
  // The active match is anchored by message id, not by index into the match
  // list: while a run is live the list can change under the same (pair, run,
  // query) scope — a revised final supersedes the active card, a new match
  // appends. A kept index would silently re-resolve to a different card and
  // outline + scroll to it with no cycle press; an anchored id keeps pointing
  // at the card the reader chose and deactivates when that card leaves the
  // list.
  const [activeMatchId, setActiveMatchId] = useState<string | null>(null)
  // Bumped on every cycle call, even when the active match doesn't change (a
  // single match), so the scroll effect below re-runs on every press.
  const [matchCycle, setMatchCycle] = useState(0)
  // Cycle keys pressed while the deferred list lags the typed query are
  // parked here as a signed step count (see cycleMatch) and replayed by the
  // render where the list catches up, so presses never index the stale list
  // and a fast double-press composes instead of keeping only the last one.
  const [pendingMatchSteps, setPendingMatchSteps] = useState(0)
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

  // The active match belongs to the match list of one (pair, run, query).
  // When any of those change the anchor must be cleared in that same commit:
  // an effect only resets it after the commit, leaving one render where the
  // stale anchor outlines and scrolls to a card the reader never cycled to.
  // Resetting during render re-renders before anything is committed. Query
  // edits come from this component, but run/pair switches arrive from
  // siblings through the store, so no single event handler can batch the
  // reset — the scope check here covers every source. Scoped to the deferred
  // query, which is what the match list derives from, so the anchor only
  // resets when the list does.
  const matchScope = `${pair.id}::${viewingRun?.id ?? 'live'}::${listQuery}`
  const [cycledMatchScope, setCycledMatchScope] = useState(matchScope)
  if (cycledMatchScope !== matchScope) {
    setCycledMatchScope(matchScope)
    setActiveMatchId(null)
    // A parked press belongs to the scope it was pressed in, so it dies with
    // the old scope too. A query catch-up is safe to clear here as well: the
    // parked presses replay later in this same render pass and supersede the
    // reset, while a run or pair switch lands on a render where the deferred
    // query still lags, so the replay below never fires for it.
    setPendingMatchSteps(0)
  }

  const roleFilteredMessages = useMemo(() => {
    const messages = viewingRun ? viewingRun.messages : pair.messages
    if (messageFilter === 'all') return messages
    return messages.filter((msg) => msg.from === 'human' || msg.from === messageFilter)
  }, [pair.messages, viewingRun, messageFilter])

  const searching = searchOpen && listQuery.trim().length > 0
  // Dedup before the search filter, so which drafts collapse never depends on
  // the query: search keeps the same finals as the unsearched view, a draft
  // hidden without a query stays hidden with one, and the final's drop marker
  // survives.
  const { deduplicatedConsoleMessages, dropCountByMessageId } = useMemo(() => {
    const { kept, droppedBeforeId } = collapseWithDropCounts<Message>(roleFilteredMessages)
    return { deduplicatedConsoleMessages: kept, dropCountByMessageId: droppedBeforeId }
  }, [roleFilteredMessages])

  // Count and cycle only over matches that render as highlighted, anchored
  // cards, so collapsed-away messages never inflate the count and Enter never
  // lands on a message with no `console-msg-<id>` element. The count also
  // decides listing — the same rendered-text count that drives the badge and
  // Enter cycling, so a message whose only raw-source hit is a link URL or
  // another non-rendered syntax never stays listed without a highlight — and
  // counting once here keeps the per-keystroke pass single.
  const searchMatches = useMemo(() => {
    if (!searching) return [] as Array<{ id: string; count: number }>
    const out: Array<{ id: string; count: number }> = []
    for (const m of deduplicatedConsoleMessages) {
      const count = countSearchHighlights(m, listQuery)
      if (count > 0) out.push({ id: m.id, count })
    }
    return out
  }, [deduplicatedConsoleMessages, searching, listQuery])

  const matchIds = useMemo(() => new Set(searchMatches.map((x) => x.id)), [searchMatches])
  const consoleMessages = useMemo(
    () =>
      searching
        ? deduplicatedConsoleMessages.filter((m) => matchIds.has(m.id))
        : deduplicatedConsoleMessages,
    [deduplicatedConsoleMessages, searching, matchIds]
  )

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

  // The render where the deferred list catches up applies the parked presses
  // to the final list, replaying each step from the list's start — the anchor
  // reset above cleared where the reader was, so a composed press lands where
  // the same presses from scratch would. Render-time for the same reason as
  // the scope reset above: an effect would commit one paint where the
  // keypresses the reader made outlined nothing.
  if (pendingMatchSteps !== 0 && listQuery === searchQuery) {
    const dir: 1 | -1 = pendingMatchSteps > 0 ? 1 : -1
    setPendingMatchSteps(0)
    let next = -1
    for (let step = 0; step < Math.abs(pendingMatchSteps); step++) {
      next = nextMatchIndex(next, matchingMessageIds.length, dir)
    }
    setActiveMatchId(next >= 0 ? matchingMessageIds[next] : null)
    setMatchCycle((cycle) => cycle + 1)
  }

  // A card superseded out of the match list (a live final superseding the
  // active one) deactivates here, in the same render that removes it — no
  // stale outline survives and nothing scrolls. When the anchored card stays
  // listed, appends around it never move the activation.
  const activeMessageId =
    activeMatchId !== null && matchIds.has(activeMatchId) ? activeMatchId : null

  const cycleMatch = useCallback(
    (dir: 1 | -1): void => {
      // The deferred list can lag the typed query by a keystroke. Cycling it
      // would outline and scroll to a card the final query may not match,
      // and the scope reset above would then drop the activation — park the
      // press instead and let the caught-up render apply it.
      if (listQuery !== searchQuery) {
        setPendingMatchSteps((steps) => steps + dir)
        return
      }
      // An anchored card no longer in the list resolves to index -1, so
      // cycling from it restarts from the list's first (or, backwards, last)
      // match.
      const current = activeMatchId !== null ? matchingMessageIds.indexOf(activeMatchId) : -1
      const next = nextMatchIndex(current, matchingMessageIds.length, dir)
      setActiveMatchId(next >= 0 ? matchingMessageIds[next] : null)
      setMatchCycle((cycle) => cycle + 1)
    },
    [activeMatchId, matchingMessageIds, listQuery, searchQuery]
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
    setActiveMatchId(null)
    setPendingMatchSteps(0)
  }, [])

  // Cmd/Ctrl+F opens transcript search while this console is mounted. The key
  // itself is registered in App.tsx's shortcut map (exact platform modifier,
  // preventDefault, listed in the shortcuts sheet) and reaches the console
  // through this event. flushSync commits the input before the frame's rAF
  // fires — a default-priority update from this raw listener could otherwise
  // land after it, leaving the ref null and the input unfocused. The clear-
  // history confirm is this component's own aria-modal dialog; App's modal
  // flags structurally cannot see it, so the listener itself must decline
  // while it is open rather than pull focus out of the dialog.
  useEffect(() => {
    const openSearch = (): void => {
      if (confirmClear) return
      flushSync(() => {
        setSearchOpen(true)
      })
      requestAnimationFrame(() => searchInputRef.current?.focus())
    }
    window.addEventListener('app:console:open-search', openSearch)
    return () => window.removeEventListener('app:console:open-search', openSearch)
  }, [confirmClear])

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

  // Follow new output only while the reader is already at the bottom, never
  // while reading an archived run, and never mid-search: an Enter-scrolled
  // match within the stick threshold of the filtered list's bottom leaves
  // stickToBottom on, and every streamed message would otherwise yank the
  // reader off the outlined match until they scroll by hand.
  useEffect(() => {
    if (isViewingArchived || searching || !stickToBottomRef.current) return
    const el = scrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [
    isViewingArchived,
    searching,
    pair.status,
    pair.messages.length,
    pair.currentTurnCard?.updatedAt
  ])

  // The docked composer grows with multi-line input and the review bar; the
  // transcript shrinks by the same amount, which would hide the newest lines
  // from a reader following the bottom. Re-pin on resize, under the same rules.
  useEffect(() => {
    const el = scrollRef.current
    if (!el || isViewingArchived || searching || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(() => {
      if (stickToBottomRef.current) el.scrollTop = el.scrollHeight
    })
    observer.observe(el)
    return () => observer.disconnect()
  }, [isViewingArchived, searching])

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
              searchQuery={searching ? listQuery : undefined}
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
    listQuery,
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
  const caretClass =
    inputLocked || isSubmittingTask
      ? 'text-muted-foreground-faint'
      : inputFocused
        ? 'text-foreground tty-blink'
        : 'text-muted-foreground-faint'

  const taskInputRow = !isViewingArchived ? (
    <form
      onSubmit={handleTaskSubmit}
      className={cn(inputLocked ? 'cursor-not-allowed opacity-60' : 'cursor-text')}
      onClick={() => {
        if (!inputLocked) inputRef.current?.focus()
      }}
      data-testid="pair-task-input"
      aria-disabled={inputLocked}
    >
      <div className="flex items-start gap-2 font-mono text-[12px] leading-relaxed">
        <span
          aria-hidden
          className={cn(
            'select-none',
            inputLocked ? 'text-muted-foreground-faint' : 'text-muted-foreground'
          )}
        >
          {'>'}
        </span>
        {/* Long input scrolls inside the dock instead of squeezing the transcript. */}
        <div className="max-h-[40vh] flex-1 min-w-0 overflow-y-auto scrollbar-thin">
          <div className="relative">
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
                  <span className={cn('select-none ml-[1px]', caretClass)}>▍</span>
                </>
              ) : (
                <>
                  <span className={cn('select-none mr-[2px]', caretClass)}>▍</span>
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
              onFocus={() => setInputFocused(true)}
              onBlur={() => setInputFocused(false)}
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
                'absolute inset-0 w-full h-full resize-none overflow-hidden bg-transparent border-0 outline-none p-0 font-mono text-[12px] leading-relaxed text-transparent caret-transparent',
                inputLocked && 'pointer-events-none'
              )}
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
      </div>
      {(fileContexts.size > 0 || skillContexts.size > 0) && (
        <div className="mt-1.5 flex flex-wrap gap-1 pl-[2ch]">
          {Array.from(skillContexts.keys())
            .filter((name) => taskInput.includes(`/${name}`))
            .map((name) => (
              <Chip
                key={`skill-${name}`}
                title={t('console.attachedSkill', { name })}
                className="normal-case tracking-normal"
              >
                <span className="text-muted-foreground-faint">/</span>
                <span className="truncate max-w-[28ch]">{name}</span>
              </Chip>
            ))}
          {Array.from(fileContexts.keys())
            .filter((path) => taskInput.includes(`@${path}`))
            .map((path) => (
              <Chip
                key={`file-${path}`}
                title={t('console.attachedFile', { path })}
                className="normal-case tracking-normal"
              >
                <span className="text-muted-foreground-faint">@</span>
                <span className="truncate max-w-[28ch]">{path}</span>
              </Chip>
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

  const reviewDock =
    pair.status !== 'Awaiting Human Review' ? null : isViewingArchived ? (
      // While viewing an archived run, don't render the approve/reject
      // controls (they act on the live pair, not the run on screen);
      // surface a clear one-click return to the pending plan instead.
      <button
        type="button"
        onClick={() => setViewingRunId(null)}
        className="flex w-full items-center gap-1.5 rounded-sm border border-state-running/40 bg-state-running/[0.06] p-3 text-left font-mono text-[11px] state-running transition-colors hover:bg-state-running/[0.1]"
      >
        <span aria-hidden>▸</span>
        <span className="font-bold uppercase tracking-[0.14em]">
          {t('planReview.reviewPending')}
        </span>
        <span className="ml-auto text-muted-foreground">{t('planReview.returnToReview')} →</span>
      </button>
    ) : (
      <PlanReviewBar pair={pair} />
    )

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
            <BackToCurrentButton
              onClick={() => setViewingRunId(null)}
              data-testid="console-back-to-live-btn"
            />
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
              onFilterChange={(filter) => {
                setMessageFilter(filter)
                // The anchor belongs to the role-filtered match list: reset
                // it in the same batched commit as the filter change, or the
                // stale anchor would briefly point at (and scroll to) a card
                // in the new list that the reader never cycled to. A cycle
                // press parked on the lagging list belongs to the unfiltered
                // list the same way and dies with the switch.
                setActiveMatchId(null)
                setPendingMatchSteps(0)
              }}
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
          <div className="flex items-center gap-0.5">
            {!isViewingArchived && pair.messages.length > 0 && !isRunning && (
              <IconButton
                onClick={handleClearMessages}
                aria-label={t('console.clearHistory')}
                destructive
                data-testid="console-clear-btn"
              >
                <Trash2 size={12} />
              </IconButton>
            )}
            <IconButton
              onClick={() => {
                setSearchOpen(true)
                requestAnimationFrame(() => searchInputRef.current?.focus())
              }}
              aria-label={t('console.searchOpen')}
              aria-expanded={searchOpen}
              data-testid="console-search-toggle"
            >
              <Search size={12} />
            </IconButton>
          </div>
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
          <div className="flex shrink-0 items-center gap-0.5">
            <IconButton
              onClick={() => cycleMatch(-1)}
              disabled={matchingMessageIds.length === 0}
              aria-label={t('console.searchPrev')}
              data-testid="console-search-prev"
            >
              <ChevronUp size={12} />
            </IconButton>
            <IconButton
              onClick={() => cycleMatch(1)}
              disabled={matchingMessageIds.length === 0}
              aria-label={t('console.searchNext')}
              data-testid="console-search-next"
            >
              <ChevronDown size={12} />
            </IconButton>
            <IconButton
              onClick={closeSearch}
              aria-label={t('console.searchClose')}
              data-testid="console-search-close"
            >
              <X size={12} />
            </IconButton>
          </div>
        </div>
      )}

      {/* Message area */}
      <div className="relative flex min-h-0 flex-1 flex-col">
        <div
          ref={scrollRef}
          data-testid="console-transcript"
          className="flex-1 min-h-0 overflow-y-auto scrollbar-thin"
        >
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
                              className="font-mono"
                              icon={
                                visibleCurrentTurnCard.activity.phase === 'stalled' ? (
                                  <XCircle size={11} />
                                ) : (
                                  <Square size={10} />
                                )
                              }
                              onClick={() => {
                                void handleStopTurn(visibleCurrentTurnCard)
                              }}
                              disabled={isStoppingTurn}
                              title={t('console.stopTurn')}
                              data-testid="console-stop-turn-btn"
                            >
                              {visibleCurrentTurnCard.activity.phase === 'stalled'
                                ? t('pair.killProcess')
                                : t('console.stopTurn')}
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
                            className={cn(
                              'inline-flex h-[1em] w-[1.4ch] items-center justify-center',
                              pair.turn === 'mentor' ? 'role-mentor' : 'role-executor'
                            )}
                          >
                            <span className="tty-spin">✻</span>
                          </span>
                          {t('pair.thinking')}
                        </span>
                      </TerminalBlock>
                    )}
                  </AnimatePresence>
                )}
              </>
            )}
          </div>
        </div>
        <ScrollToBottomButton
          scrollRef={scrollRef}
          dependency={`${consoleMessages.length}-${messageFilter}-${viewingRun?.id ?? 'live'}`}
        />
      </div>

      {/* Composer dock: pinned below the transcript, like a chat input. */}
      {(reviewDock || taskInputRow) && (
        <div
          data-testid="console-composer-dock"
          className="flex shrink-0 flex-col gap-2 border-t border-border bg-background px-6 py-3"
        >
          {reviewDock}
          {taskInputRow}
        </div>
      )}
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
