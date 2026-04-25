import React, { useEffect, useMemo, useRef, useState } from 'react'
import { Pause, Play, RefreshCw, RotateCcw, Terminal, Zap, XCircle } from 'lucide-react'
import { AnimatePresence, motion } from 'framer-motion'
import { cn } from '../lib/utils'
import { usePairStore, type Message, type Pair } from '../store/usePairStore'
import { TaskHistoryPanel } from './TaskHistoryPanel'
import { TimelinePanel } from './TimelinePanel'
import { ScrollToBottomButton } from './ScrollToBottomButton'
import { ErrorDetailPanel } from './ErrorDetailPanel'
import { IterationProgress } from './IterationProgress'
import { MessageFilterBar } from './MessageFilterBar'
import { GlassButton } from './ui/GlassButton'
import { ResourceMeter } from './ui/ResourceMeter'
import { buildTimeline } from '../lib/timeline'
import { isPairActive } from '../lib/pairStatus'
import { ActivityIndicator } from './ActivityIndicator'
import { isAgentExecuting } from '../lib/helpers'
import { useMinimumVisibleText } from '../hooks/useMinimumVisibleText'
import { MessageCard } from './MessageCard'
import { TurnCardView } from './TurnCardView'
import { collapseConsecutiveConsoleMessages } from '../lib/consoleMessages'

interface PairDetailProps {
  pair: Pair
  onPause: () => Promise<void>
  onResume: () => Promise<void>
  onRestoreTask: (spec: string, mentorModel: string, executorModel: string) => void
}

function PairDetail({ pair, onPause, onResume, onRestoreTask }: PairDetailProps): React.ReactNode {
  const retryTurn = usePairStore((s) => s.retryTurn)
  const killProcess = usePairStore((s) => s.killProcess)
  const isStoreBusy = usePairStore((s) => s.isLoading)
  const viewingRunId = usePairStore((s) => s.viewingRunId)
  const setViewingRunId = usePairStore((s) => s.setViewingRunId)
  const scrollRef = useRef<HTMLDivElement>(null)
  const [messageFilter, setMessageFilter] = useState<'all' | 'mentor' | 'executor'>('all')

  const viewingRun = viewingRunId
    ? (pair.runHistory.find((run) => run.id === viewingRunId) ?? null)
    : null

  const timelineData = useMemo(() => {
    const source = viewingRun ?? {
      name: pair.name,
      spec: pair.spec,
      mentorModel: pair.mentorModel,
      executorModel: pair.executorModel,
      status: pair.status,
      messages: pair.messages,
      latestAcceptance: pair.latestAcceptance,
      modifiedFiles: pair.modifiedFiles,
      currentRunStartedAt: pair.currentRunStartedAt,
      currentRunFinishedAt: pair.currentRunFinishedAt
    }

    const messages = source.messages
    if (messages.length === 0) return null

    return buildTimeline(
      messages.map((m) => ({
        id: m.id,
        timestamp: m.timestamp,
        from: m.from,
        to: m.to,
        type: m.type,
        content: m.content,
        iteration: m.iteration,
        tokenUsage: m.tokenUsage
      })),
      {
        name: 'name' in source ? source.name : pair.name,
        spec: source.spec,
        mentorModel: source.mentorModel,
        executorModel: source.executorModel,
        status: source.status,
        messages: messages,
        latestAcceptance: source.latestAcceptance,
        modifiedFiles: 'modifiedFiles' in source ? source.modifiedFiles : pair.modifiedFiles,
        currentRunStartedAt:
          'currentRunStartedAt' in source ? source.currentRunStartedAt : pair.currentRunStartedAt,
        currentRunFinishedAt:
          'currentRunFinishedAt' in source ? source.currentRunFinishedAt : pair.currentRunFinishedAt
      }
    )
  }, [pair, viewingRun])

  const consoleMessages = useMemo(() => {
    const messages = viewingRun ? viewingRun.messages : pair.messages

    if (messageFilter === 'all') return messages
    return messages.filter((msg) => msg.from === 'human' || msg.from === messageFilter)
  }, [pair.messages, viewingRun, messageFilter])

  const deduplicatedConsoleMessages = useMemo(() => {
    return collapseConsecutiveConsoleMessages<Message>(consoleMessages)
  }, [consoleMessages])

  const messageCounts = useMemo(() => {
    const allMessages = viewingRun ? viewingRun.messages : pair.messages
    return {
      mentor: allMessages.filter((msg) => msg.from === 'mentor').length,
      executor: allMessages.filter((msg) => msg.from === 'executor').length,
      all: allMessages.length
    }
  }, [pair.messages, viewingRun])

  const activeRole = pair.currentTurnCard?.role ?? pair.turn
  const reviewReason =
    pair.status === 'Paused' || pair.status === 'Awaiting Human Review'
      ? (pair.mentorActivity.detail ??
        pair.executorActivity.detail ??
        pair.currentTurnCard?.content ??
        (pair.status === 'Awaiting Human Review' ? 'Awaiting human intervention' : 'Paused'))
      : null
  const liveStatusText =
    pair.status === 'Paused' || pair.status === 'Awaiting Human Review'
      ? (reviewReason ??
        (pair.status === 'Awaiting Human Review' ? 'Awaiting human intervention' : 'Paused'))
      : pair.currentTurnCard?.content ||
        (activeRole === 'mentor'
          ? pair.mentorActivity.detail || 'Thinking...'
          : pair.executorActivity.detail || 'Working...')
  const visibleStatusText = useMinimumVisibleText(liveStatusText, pair.id)

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [pair.messages.length])

  useEffect(() => {
    if (!isPairActive(pair.status)) return
    const el = scrollRef.current
    if (!el) return

    const distanceToBottom = el.scrollHeight - el.scrollTop - el.clientHeight
    if (distanceToBottom < 160) {
      el.scrollTop = el.scrollHeight
    }
  }, [pair.status, pair.messages.length, pair.currentTurnCard?.updatedAt])

  const visibleCurrentTurnCard =
    pair.currentTurnCard &&
    (deduplicatedConsoleMessages.length === 0 ||
      deduplicatedConsoleMessages[deduplicatedConsoleMessages.length - 1].from !==
        pair.currentTurnCard.role)
      ? pair.currentTurnCard
      : null

  if (!pair || !pair.id || !pair.name) {
    console.error('[PairDetail] Invalid pair data:', pair)
    return (
      <div className="flex h-full items-center justify-center">
        <div className="text-center">
          <p className="text-lg font-semibold text-red-600">Invalid pair data</p>
          <p className="mt-2 text-sm text-muted-foreground">Please try selecting another pair</p>
        </div>
      </div>
    )
  }

  const canPause = isPairActive(pair.status)

  const handleRetryTurn = (): void => {
    retryTurn(pair.id)
  }

  const formatRunStamp = (ts?: number): string => {
    if (!ts) return 'still active'
    const d = new Date(ts)
    return `${d.getMonth() + 1}/${d.getDate()} ${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`
  }

  const runStateText =
    pair.status === 'Paused'
      ? `Paused ${formatRunStamp(pair.currentRunFinishedAt)}`
      : pair.currentRunFinishedAt
        ? `Finished ${formatRunStamp(pair.currentRunFinishedAt)}`
        : 'Still running'

  const getActivityIcon = (phase: string): string => {
    switch (phase) {
      case 'thinking':
        return '🤔'
      case 'using_tools':
        return '🔧'
      case 'responding':
        return '✍️'
      case 'waiting':
        return '⏳'
      case 'error':
        return '❌'
      default:
        return '💤'
    }
  }

  const getDuration = (startedAt: number, updatedAt: number): string => {
    const seconds = Math.floor((updatedAt - startedAt) / 1000)
    if (seconds < 60) return `${seconds}s`
    const minutes = Math.floor(seconds / 60)
    return `${minutes}m ${seconds % 60}s`
  }

  const mentorIsExecuting = isAgentExecuting(pair.mentorActivity.phase)
  const executorIsExecuting = isAgentExecuting(pair.executorActivity.phase)
  const isRunning = isPairActive(pair.status) || mentorIsExecuting || executorIsExecuting

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-1 min-h-0 overflow-hidden bg-background">
        <div className="glass-panel flex w-[28%] flex-col gap-4 overflow-y-auto border-r border-border/50 p-5 scrollbar-thin">
          <div className="flex flex-wrap items-center gap-2">
            {(pair.status === 'Paused' || pair.status === 'Awaiting Human Review') && (
              <GlassButton
                variant="secondary"
                size="sm"
                icon={<Play size={12} />}
                onClick={() => {
                  void onResume()
                }}
                disabled={isStoreBusy}
              >
                Resume Pair
              </GlassButton>
            )}
            {canPause && (
              <GlassButton
                variant="secondary"
                size="sm"
                icon={<Pause size={12} />}
                onClick={() => {
                  void onPause()
                }}
                disabled={!canPause || isStoreBusy}
              >
                Pause Pair
              </GlassButton>
            )}
            {pair.status === 'Error' && (
              <GlassButton
                variant="primary"
                size="sm"
                icon={<RotateCcw size={12} />}
                onClick={handleRetryTurn}
              >
                Retry Turn
              </GlassButton>
            )}
            {pair.automationMode === 'full-auto' && (
              <div className="flex items-center gap-1 rounded-full bg-amber-500/10 px-2 py-1 text-[10px] text-amber-600 dark:text-amber-400">
                <Zap size={10} />
                <span>Full Auto</span>
              </div>
            )}
          </div>
          {(pair.status === 'Paused' || pair.status === 'Awaiting Human Review') &&
            reviewReason && (
              <div className="rounded-2xl border border-amber-500/20 bg-amber-500/8 p-3 text-[11px] leading-relaxed text-amber-800 dark:text-amber-200">
                <div className="mb-1 text-[10px] font-semibold uppercase tracking-[0.18em] text-amber-700/80 dark:text-amber-300/80">
                  Pause reason
                </div>
                <div className="whitespace-pre-wrap break-words [overflow-wrap:anywhere]">
                  {reviewReason}
                </div>
              </div>
            )}
          {pair.status === 'Error' && (
            <ErrorDetailPanel
              error={
                (pair.mentorActivity.phase === 'error' ? pair.mentorActivity.detail : null) ??
                (pair.executorActivity.phase === 'error' ? pair.executorActivity.detail : null) ??
                'Agent encountered an error'
              }
              onRetry={handleRetryTurn}
            />
          )}
          {pair.latestAcceptance && (
            <div className="rounded-2xl border border-border/40 bg-background/25 p-4">
              <div className="mb-2 flex items-center justify-between gap-2">
                <h3 className="text-[10px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">
                  Latest Acceptance
                </h3>
                <span className="rounded-full border border-border/40 bg-background/40 px-2 py-1 text-[9px] font-bold uppercase tracking-[0.16em] text-muted-foreground/80">
                  {pair.latestAcceptance.risk}
                </span>
              </div>
              <p className="text-[12px] leading-relaxed text-foreground/85">
                {pair.latestAcceptance.summary}
              </p>
              <div className="mt-2 flex flex-wrap gap-2 text-[9px]">
                {pair.latestAcceptance.verdict && (
                  <span
                    className={cn(
                      'rounded-full border px-2 py-1 font-bold uppercase tracking-[0.16em]',
                      pair.latestAcceptance.verdict.verdict === 'pass'
                        ? 'border-emerald-500/20 bg-emerald-500/10 text-emerald-600 dark:text-emerald-300'
                        : 'border-amber-500/20 bg-amber-500/10 text-amber-600 dark:text-amber-300'
                    )}
                  >
                    {pair.latestAcceptance.verdict.verdict}
                  </span>
                )}
                <span className="rounded-full border border-border/40 bg-background/40 px-2 py-1 font-bold uppercase tracking-[0.16em] text-muted-foreground/80">
                  {pair.latestAcceptance.checks.filter((check) => check.status === 'failed').length}{' '}
                  failed checks
                </span>
                {pair.latestAcceptance.error && (
                  <span className="rounded-full border border-red-500/20 bg-red-500/10 px-2 py-1 font-bold uppercase tracking-[0.16em] text-red-600 dark:text-red-300">
                    verdict error
                  </span>
                )}
              </div>
            </div>
          )}

          <div>
            <h3 className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              Models
            </h3>
            <div className="glass-card rounded-2xl p-4 space-y-3">
              <div className="space-y-1">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <div
                      className={cn(
                        'h-1.5 w-1.5 rounded-full bg-blue-500',
                        mentorIsExecuting && 'metal-sheen-mark'
                      )}
                    />
                    <span className="text-[10px] font-bold uppercase tracking-wider text-blue-600">
                      Mentor
                    </span>
                  </div>
                  {mentorIsExecuting && (
                    <span className="text-[10px] font-mono text-blue-500 metal-sheen-text">
                      ACTIVE
                    </span>
                  )}
                </div>
                <p className="pl-4 font-mono text-xs text-muted-foreground">{pair.mentorModel}</p>
                {pair.pendingMentorModel && (
                  <p className="pl-4 text-[11px] text-amber-700 dark:text-amber-300">
                    Next task: {pair.pendingMentorModel}
                  </p>
                )}
              </div>
              <div className="space-y-1">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <div
                      className={cn(
                        'h-1.5 w-1.5 rounded-full bg-purple-500',
                        executorIsExecuting && 'metal-sheen-mark'
                      )}
                    />
                    <span className="text-[10px] font-bold uppercase tracking-wider text-purple-600">
                      Executor
                    </span>
                  </div>
                  {executorIsExecuting && (
                    <span className="text-[10px] font-mono text-purple-500 metal-sheen-text">
                      ACTIVE
                    </span>
                  )}
                </div>
                <p className="pl-4 font-mono text-xs text-muted-foreground">{pair.executorModel}</p>
                {pair.pendingExecutorModel && (
                  <p className="pl-4 text-[11px] text-amber-700 dark:text-amber-300">
                    Next task: {pair.pendingExecutorModel}
                  </p>
                )}
              </div>
            </div>
          </div>

          <div>
            <h3 className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              Run State
            </h3>
            <div className="glass-card rounded-2xl p-4 space-y-3">
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs text-muted-foreground">Run {pair.runCount}</span>
                <span className="rounded-full border border-border/50 bg-background/40 px-2 py-1 text-[10px] font-medium text-foreground">
                  {pair.status}
                </span>
              </div>
              <div className="text-[11px] text-muted-foreground">
                Started {formatRunStamp(pair.currentRunStartedAt)} · {runStateText}
              </div>
              <IterationProgress current={pair.iterations} max={pair.maxIterations} />
            </div>
          </div>

          <TaskHistoryPanel
            runHistory={pair.runHistory}
            viewingRunId={viewingRunId}
            onSelectTask={(runId) => setViewingRunId(runId)}
            onBackToCurrent={() => setViewingRunId(null)}
            onRestoreTask={(run) => onRestoreTask(run.spec, run.mentorModel, run.executorModel)}
            timeline={viewingRunId ? timelineData : null}
          />

          <TimelinePanel timeline={timelineData} />
        </div>

        <div className="flex w-[46%] flex-col bg-muted/10">
          <div className="flex h-10 shrink-0 items-center gap-2 border-b border-border/50 px-4 font-mono text-[11px] text-muted-foreground bg-background/50">
            <Terminal size={13} />
            <span className="uppercase tracking-widest font-bold">
              {viewingRunId ? 'Task History' : 'Session Console'}
            </span>
            {viewingRunId && (
              <span className="text-[9px] text-muted-foreground/50 ml-1">
                · viewing archived run
              </span>
            )}
            <div className="ml-auto flex items-center gap-3">
              {!viewingRunId && (
                <MessageFilterBar
                  activeFilter={messageFilter}
                  onFilterChange={setMessageFilter}
                  counts={messageCounts}
                />
              )}
              <div className="flex items-center gap-1.5">
                <div
                  className={cn(
                    'h-2 w-2 rounded-full',
                    isRunning ? 'bg-green-500 animate-pulse' : 'bg-muted-foreground/30'
                  )}
                />
                <span className="text-[10px]">{isRunning ? 'SYSTEM ONLINE' : 'SYSTEM IDLE'}</span>
              </div>
            </div>
          </div>

          <div className="flex h-14 shrink-0 items-center gap-4 border-b border-border/30 bg-muted/5 px-4">
            <div
              className={cn(
                'flex flex-col gap-0.5 transition-all duration-300',
                activeRole === 'mentor' ? 'opacity-100 scale-100' : 'opacity-40 scale-95 grayscale'
              )}
            >
              <div className="flex items-center gap-2">
                <span className="text-[10px] font-bold text-blue-600 dark:text-blue-400 tracking-tighter">
                  MENTOR
                </span>
                <span className={cn('text-xs', mentorIsExecuting && 'metal-sheen-text')}>
                  {getActivityIcon(pair.mentorActivity.phase)}
                </span>
                {mentorIsExecuting && (
                  <span className="text-[9px] font-mono text-muted-foreground/70">
                    {getDuration(pair.mentorActivity.startedAt, pair.mentorActivity.updatedAt)}
                  </span>
                )}
              </div>
              <span
                className={cn(
                  'max-w-[140px] truncate text-[11px] font-medium text-foreground',
                  mentorIsExecuting && 'metal-sheen-text'
                )}
              >
                {pair.mentorActivity.label}
              </span>
            </div>

            <div className="h-6 w-px bg-border/50" />

            <div
              className={cn(
                'flex flex-col gap-0.5 transition-all duration-300',
                activeRole === 'executor'
                  ? 'opacity-100 scale-100'
                  : 'opacity-40 scale-95 grayscale'
              )}
            >
              <div className="flex items-center gap-2">
                <span className="text-[10px] font-bold text-purple-600 dark:text-purple-400 tracking-tighter">
                  EXECUTOR
                </span>
                <span className={cn('text-xs', executorIsExecuting && 'metal-sheen-text')}>
                  {getActivityIcon(pair.executorActivity.phase)}
                </span>
                {executorIsExecuting && (
                  <span className="text-[9px] font-mono text-muted-foreground/70">
                    {getDuration(pair.executorActivity.startedAt, pair.executorActivity.updatedAt)}
                  </span>
                )}
              </div>
              <span
                className={cn(
                  'max-w-[140px] truncate text-[11px] font-medium text-foreground',
                  executorIsExecuting && 'metal-sheen-text'
                )}
              >
                {pair.executorActivity.label}
              </span>
            </div>

            <div className="ml-auto flex items-center gap-3">
              {pair.status === 'Awaiting Human Review' && (
                <div className="max-w-[320px] rounded-full border border-amber-500/20 bg-amber-500/10 px-3 py-1.5 text-[10px] font-semibold uppercase tracking-[0.18em] text-amber-700 dark:text-amber-300">
                  <span className="block truncate">Human review required</span>
                  <span className="mt-1 block truncate text-[9px] font-medium normal-case tracking-normal text-amber-700/80 dark:text-amber-300/80">
                    {visibleStatusText}
                  </span>
                </div>
              )}
              {isRunning && (
                <div className="flex items-center gap-2 rounded-full border border-border/50 bg-background/50 px-3 py-1.5 shadow-sm animate-in fade-in slide-in-from-right-2">
                  <RefreshCw size={12} className="animate-spin text-primary/60" />
                  <span className="text-[10px] font-bold text-muted-foreground truncate max-w-[150px]">
                    {visibleStatusText}
                  </span>
                </div>
              )}
            </div>
          </div>

          <div className="relative flex min-h-0 flex-1 flex-col bg-background/20">
            <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto scrollbar-thin relative">
              <div className="flex flex-col gap-4 p-6 pb-20">
                {deduplicatedConsoleMessages.length === 0 && !pair.currentTurnCard ? (
                  <div className="flex h-[400px] flex-col items-center justify-center space-y-4 opacity-40">
                    <div className="h-12 w-12 rounded-full border-2 border-dashed border-muted-foreground/30 animate-[spin_10s_linear_infinite]" />
                    <div className="text-center space-y-1">
                      <p className="text-xs font-bold uppercase tracking-[0.2em]">Fresh Session</p>
                      <p className="text-[10px]">Awaiting first agent instruction</p>
                    </div>
                  </div>
                ) : (
                  <div className="flex flex-col gap-4">
                    {deduplicatedConsoleMessages.map((msg: Message) => (
                      <MessageCard msg={msg} key={msg.id} />
                    ))}
                  </div>
                )}
                {(isPairActive(pair.status) || visibleCurrentTurnCard) && (
                  <div className="pt-1">
                    <AnimatePresence mode="popLayout">
                      {visibleCurrentTurnCard ? (
                        <motion.div
                          key={visibleCurrentTurnCard.id}
                          initial={false}
                          animate={{ opacity: 1, y: 0 }}
                          exit={{ opacity: 0, transition: { duration: 0.15 } }}
                          transition={{ duration: 0.15 }}
                        >
                          <TurnCardView card={visibleCurrentTurnCard} />
                          {visibleCurrentTurnCard.activity.phase === 'stalled' && (
                            <div className="mt-2 flex items-center gap-2">
                              <GlassButton
                                variant="destructive"
                                size="sm"
                                className="w-full gap-1.5"
                                onClick={() => {
                                  void killProcess(pair.id, visibleCurrentTurnCard.role)
                                }}
                              >
                                <XCircle size={12} />
                                Kill Process
                              </GlassButton>
                            </div>
                          )}
                        </motion.div>
                      ) : (
                        <motion.div
                          key="active-placeholder"
                          initial={{ opacity: 0 }}
                          animate={{ opacity: 1 }}
                          exit={{ opacity: 0, transition: { duration: 0.15 } }}
                          transition={{ duration: 0.15 }}
                        >
                          <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-glass-panel text-sm text-muted-foreground">
                            <ActivityIndicator
                              activity={
                                pair.turn === 'mentor' ? pair.mentorActivity : pair.executorActivity
                              }
                            />
                          </div>
                        </motion.div>
                      )}
                    </AnimatePresence>
                  </div>
                )}
              </div>
            </div>
            <ScrollToBottomButton
              scrollRef={scrollRef}
              dependency={`${consoleMessages.length}-${messageFilter}-${viewingRunId ?? 'live'}`}
            />
          </div>
        </div>

        <div className="glass-panel flex w-[26%] flex-col gap-4 overflow-y-auto p-5 scrollbar-thin">
          <div>
            <h3 className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              System Resources
            </h3>
            <div className="glass-card rounded-2xl p-3">
              <ResourceMeter cpu={pair.cpuUsage} mem={pair.memUsage} />
            </div>
          </div>
          <div>
            <h3 className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              Recent Activity
            </h3>
            <div className="glass-card rounded-2xl p-3 space-y-2">
              <div className="flex items-center gap-2 text-xs">
                <div
                  className={cn(
                    'h-1.5 w-1.5 rounded-full',
                    pair.mentorActivity.phase === 'error'
                      ? 'bg-red-500'
                      : pair.mentorActivity.phase !== 'idle'
                        ? 'animate-pulse bg-blue-500'
                        : 'bg-blue-500/50'
                  )}
                />
                <span className="text-muted-foreground/70">MENTOR:</span>
                <span className="truncate text-blue-600 dark:text-blue-400">
                  {pair.mentorActivity.label}
                </span>
              </div>
              <div className="flex items-center gap-2 text-xs">
                <div
                  className={cn(
                    'h-1.5 w-1.5 rounded-full',
                    pair.executorActivity.phase === 'error'
                      ? 'bg-red-500'
                      : pair.executorActivity.phase !== 'idle'
                        ? 'animate-pulse bg-purple-500'
                        : 'bg-purple-500/50'
                  )}
                />
                <span className="text-muted-foreground/70">EXEC:</span>
                <span className="truncate text-purple-600 dark:text-purple-400">
                  {pair.executorActivity.label}
                </span>
              </div>
              {pair.mentorActivity.detail && (
                <div className="truncate pl-4 text-[10px] text-muted-foreground/50">
                  {pair.mentorActivity.detail}
                </div>
              )}
            </div>
          </div>
          <div>
            <h3 className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              Modified Files
            </h3>
            <div className="glass-card rounded-2xl p-3">
              {!pair.gitTracking.available ? (
                <div className="font-mono text-xs text-amber-600/70 dark:text-amber-400/70">
                  Git tracking unavailable for this workspace
                </div>
              ) : pair.modifiedFiles.length === 0 ? (
                <div className="font-mono text-xs text-muted-foreground/60">
                  No files modified in this run yet
                </div>
              ) : (
                <div className="space-y-1">
                  {pair.modifiedFiles.map((file, index) => (
                    <div
                      key={index}
                      className="flex items-center gap-1 truncate font-mono text-xs text-muted-foreground"
                      title={file.path}
                    >
                      <span className="text-purple-500/60">{file.status}</span>
                      <span className="truncate">{file.displayPath}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

export default PairDetail
