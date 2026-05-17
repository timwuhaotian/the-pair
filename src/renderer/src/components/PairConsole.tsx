import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Trash2, XCircle } from 'lucide-react'
import { AnimatePresence } from 'framer-motion'
import { cn } from '../lib/utils'
import { usePairStore, type Message, type Pair } from '../store/usePairStore'
import { ScrollToBottomButton } from './ScrollToBottomButton'
import { MessageFilterBar } from './MessageFilterBar'
import { GlassButton } from './ui/GlassButton'
import { isPairActive } from '../lib/pairStatus'
import { isAgentExecuting } from '../lib/helpers'
import { MessageCard } from './MessageCard'
import { TurnCardView } from './TurnCardView'
import { SystemBanner } from './SystemBanner'
import { TerminalBlock } from './terminal/TerminalBlock'
import { collapseConsecutiveConsoleMessages } from '../lib/consoleMessages'
import { FileMention } from './FileMention'
import { SkillMention } from './SkillMention'
import { type FileContexts } from '../lib/fileMentions'
import { composeFinalSpec, type SkillContexts } from '../lib/skillMentions'

interface PairConsoleProps {
  pair: Pair
  className?: string
}

function PairConsole({ pair, className }: PairConsoleProps): React.ReactNode {
  const { t } = useTranslation()
  const killProcess = usePairStore((s) => s.killProcess)
  const setMessages = usePairStore((s) => s.setMessages)
  const assignTask = usePairStore((s) => s.assignTask)
  const isLoading = usePairStore((s) => s.isLoading)
  const viewingRunId = usePairStore((s) => s.viewingRunId)
  const scrollRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const [messageFilter, setMessageFilter] = useState<'all' | 'mentor' | 'executor'>('all')
  const [taskInput, setTaskInput] = useState('')
  const [composingText, setComposingText] = useState('')
  const [isSubmittingTask, setIsSubmittingTask] = useState(false)
  const [fileContexts, setFileContexts] = useState<FileContexts>(new Map())
  const [skillContexts, setSkillContexts] = useState<SkillContexts>(new Map())

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

  const viewingRun = viewingRunId
    ? (pair.runHistory.find((run) => run.id === viewingRunId) ?? null)
    : null

  const consoleMessages = useMemo(() => {
    const messages = viewingRun ? viewingRun.messages : pair.messages
    if (messageFilter === 'all') return messages
    return messages.filter((msg) => msg.from === 'human' || msg.from === messageFilter)
  }, [pair.messages, viewingRun, messageFilter])

  const deduplicatedConsoleMessages = useMemo(
    () => collapseConsecutiveConsoleMessages<Message>(consoleMessages),
    [consoleMessages]
  )

  const messageCounts = useMemo(() => {
    const allMessages = viewingRun ? viewingRun.messages : pair.messages
    return {
      mentor: allMessages.filter((msg) => msg.from === 'mentor').length,
      executor: allMessages.filter((msg) => msg.from === 'executor').length,
      all: allMessages.length
    }
  }, [pair.messages, viewingRun])

  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = scrollRef.current.scrollHeight
  }, [pair.messages.length])

  useEffect(() => {
    if (!isPairActive(pair.status)) return
    const el = scrollRef.current
    if (!el) return
    const distanceToBottom = el.scrollHeight - el.scrollTop - el.clientHeight
    if (distanceToBottom < 160) el.scrollTop = el.scrollHeight
  }, [pair.status, pair.messages.length, pair.currentTurnCard?.updatedAt])

  const visibleCurrentTurnCard =
    pair.currentTurnCard &&
    (deduplicatedConsoleMessages.length === 0 ||
      deduplicatedConsoleMessages[deduplicatedConsoleMessages.length - 1].from !==
        pair.currentTurnCard.role)
      ? pair.currentTurnCard
      : null

  const renderedChat = useMemo(() => {
    const nodes: React.ReactNode[] = []
    let lastIteration = -1
    for (const msg of deduplicatedConsoleMessages) {
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

      const isMentor = msg.from === 'mentor'
      const isExecutor = msg.from === 'executor'
      const isHuman = msg.from === 'human'
      // Mission spec (the launching task at iteration 0) reads as a brief — let it span the full column.
      const isMission = isHuman && !(msg.type === 'feedback' && msg.iteration > 0)

      const maxWidthClass = isMission ? 'w-full' : isHuman ? 'max-w-[85%]' : 'max-w-[80%]'
      const alignClass = isMission
        ? 'justify-stretch'
        : isMentor
          ? 'justify-start'
          : isExecutor
            ? 'justify-end'
            : 'justify-center'

      nodes.push(
        <div key={msg.id} className={cn('flex', alignClass)}>
          <div className={cn(maxWidthClass)}>
            <MessageCard msg={msg} />
          </div>
        </div>
      )
    }
    return nodes
  }, [deduplicatedConsoleMessages, pair.maxIterations])

  const handleClearMessages = (): void => {
    setMessages(pair.id, [])
  }

  const submitTask = async (): Promise<void> => {
    const spec = taskInput.trim()
    if (!spec || isLoading || isSubmittingTask) return

    setIsSubmittingTask(true)
    try {
      const finalSpec = composeFinalSpec(spec, fileContexts, skillContexts, pair.executorProvider)
      await assignTask(pair.id, finalSpec)
      setTaskInput('')
      setFileContexts(new Map())
      setSkillContexts(new Map())
      requestAnimationFrame(() => {
        const el = scrollRef.current
        if (el) el.scrollTop = el.scrollHeight
        inputRef.current?.focus()
      })
    } catch {
      // Store already handles errors
    } finally {
      setIsSubmittingTask(false)
    }
  }

  const handleTaskSubmit = (e: React.FormEvent): void => {
    e.preventDefault()
    void submitTask()
  }

  const handleInputKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>): void => {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault()
      void submitTask()
    }
  }

  const hasText = taskInput.length > 0 || composingText.length > 0

  const taskInputRow = !viewingRunId ? (
    <form
      onSubmit={handleTaskSubmit}
      className="mt-2 cursor-text"
      onClick={() => inputRef.current?.focus()}
      data-testid="pair-task-input"
    >
      <div className="flex items-baseline gap-2 font-mono text-[12px] leading-relaxed">
        <span aria-hidden className="state-running select-none">
          {'>'}
        </span>
        <div className="relative flex-1 min-w-0">
          <div
            aria-hidden
            className="whitespace-pre-wrap break-words font-mono text-[12px] text-foreground"
          >
            {hasText ? (
              <>
                <span>{taskInput}</span>
                {composingText && (
                  <span className="text-muted-foreground underline decoration-dotted">
                    {composingText}
                  </span>
                )}
                <span
                  className={cn(
                    'state-running select-none ml-[1px]',
                    isSubmittingTask ? '' : 'tty-blink'
                  )}
                >
                  ▍
                </span>
              </>
            ) : (
              <>
                <span
                  className={cn(
                    'state-running select-none mr-[2px]',
                    isSubmittingTask ? '' : 'tty-blink'
                  )}
                >
                  ▍
                </span>
                <span className="text-muted-foreground-faint">
                  {t('console.taskInputPlaceholder')}
                </span>
              </>
            )}
          </div>
          <textarea
            ref={inputRef}
            rows={1}
            value={taskInput}
            onChange={(e) => setTaskInput(e.target.value)}
            onKeyDown={handleInputKeyDown}
            onCompositionStart={(e) => setComposingText(e.data || '')}
            onCompositionUpdate={(e) => setComposingText(e.data || '')}
            onCompositionEnd={() => setComposingText('')}
            aria-label={t('console.taskInputPlaceholder')}
            autoComplete="off"
            spellCheck={false}
            disabled={isSubmittingTask}
            className="absolute inset-0 w-full h-full resize-none overflow-hidden bg-transparent border-0 outline-none p-0 font-mono text-[12px] leading-relaxed text-transparent"
            style={{ caretColor: 'transparent' }}
          />
          {pair.directory && (
            <FileMention
              textareaRef={inputRef}
              onChange={setTaskInput}
              directory={pair.directory}
              pairId={pair.id}
              onFileSelect={handleFileSelect}
            />
          )}
          <SkillMention
            textareaRef={inputRef}
            onChange={setTaskInput}
            projectDir={pair.directory}
            onSkillSelect={handleSkillSelect}
          />
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

  const mentorIsExecuting = isAgentExecuting(pair.mentorActivity.phase)
  const executorIsExecuting = isAgentExecuting(pair.executorActivity.phase)
  const isRunning = isPairActive(pair.status) || mentorIsExecuting || executorIsExecuting

  return (
    <div className={cn('flex h-full flex-col bg-background', className)}>
      {/* Header bar */}
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border bg-background px-4 font-mono text-[11px] text-muted-foreground">
        <span aria-hidden className="select-none text-foreground/75">
          {'>_'}
        </span>
        <span className="uppercase tracking-[0.14em] font-bold text-foreground/90">
          {viewingRunId ? t('history.title') : t('pair.sessionConsole')}
        </span>
        {viewingRunId && (
          <span className="text-muted-foreground-faint">· {t('console.viewingArchived')}</span>
        )}
        <span className="text-muted-foreground-faint">·</span>
        <span className="tabular-nums">
          iter {pair.iterations}/{pair.maxIterations}
        </span>
        <div className="ml-auto flex items-center gap-3">
          {!viewingRunId && (
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
          {!viewingRunId && pair.messages.length > 0 && !isRunning && (
            <button
              onClick={handleClearMessages}
              title={t('console.clearHistory')}
              className="p-0.5 text-muted-foreground-faint hover:text-state-error transition-colors cursor-pointer"
            >
              <Trash2 size={12} />
            </button>
          )}
        </div>
      </div>

      {/* Message area */}
      <div className="relative flex min-h-0 flex-1 flex-col">
        <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto scrollbar-thin">
          <div className="flex w-full flex-col gap-2 px-6 py-6">
            {deduplicatedConsoleMessages.length === 0 && !pair.currentTurnCard ? (
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
                {(isPairActive(pair.status) || visibleCurrentTurnCard) && (
                  <AnimatePresence mode="popLayout">
                    {visibleCurrentTurnCard ? (
                      <div key={visibleCurrentTurnCard.id} className="space-y-1">
                        <TurnCardView card={visibleCurrentTurnCard} />
                        {visibleCurrentTurnCard.activity.phase === 'stalled' && (
                          <div className="pl-[3ch]">
                            <GlassButton
                              variant="destructive"
                              size="sm"
                              className="gap-1.5"
                              onClick={() => {
                                void killProcess(pair.id, visibleCurrentTurnCard.role)
                              }}
                            >
                              <XCircle size={11} />
                              {t('pair.killProcess')}
                            </GlassButton>
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
                          {t('common.thinking')}
                        </span>
                      </TerminalBlock>
                    )}
                  </AnimatePresence>
                )}
                {taskInputRow}
              </>
            )}
          </div>
        </div>
        <ScrollToBottomButton
          scrollRef={scrollRef}
          dependency={`${consoleMessages.length}-${messageFilter}-${viewingRunId ?? 'live'}`}
        />
      </div>
    </div>
  )
}

export default PairConsole
