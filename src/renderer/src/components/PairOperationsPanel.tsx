import React, { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, Loader2, Pause, Play, RotateCcw, SlidersHorizontal, Zap } from 'lucide-react'
import { cn } from '../lib/utils'
import { usePairStore, type Pair } from '../store/usePairStore'
import { TaskHistoryPanel } from './TaskHistoryPanel'
import { TimelinePanel } from './TimelinePanel'
import { ErrorDetailPanel } from './ErrorDetailPanel'
import { IterationProgress } from './IterationProgress'
import { GlassButton } from './ui/GlassButton'
import { ResourceMeter } from './ui/ResourceMeter'
import { buildTimeline } from '../lib/timeline'
import { isPairActive } from '../lib/pairStatus'
import { isAgentExecuting } from '../lib/helpers'
import { TerminalDivider } from './terminal/TerminalDivider'
import { modifierLabel, shiftLabel } from '../lib/shortcuts'
import { FileDiffModal } from './FileDiffModal'
import { ModelPicker } from './ModelPicker'

interface PairOperationsPanelProps {
  pair: Pair
  onPause: () => Promise<void>
  onResume: () => Promise<void>
  onRestoreTask: (spec: string, mentorModel: string, executorModel: string) => void
  className?: string
}

function SectionHeader({ label }: { label: string }): React.ReactNode {
  return <TerminalDivider label={label} className="py-0.5" />
}

function PairOperationsPanel({
  pair,
  onPause,
  onResume,
  onRestoreTask,
  className
}: PairOperationsPanelProps): React.ReactNode {
  const { t } = useTranslation()
  const retryTurn = usePairStore((s) => s.retryTurn)
  const isStoreBusy = usePairStore((s) => s.isLoading)
  const viewingRunId = usePairStore((s) => s.viewingRunId)
  const setViewingRunId = usePairStore((s) => s.setViewingRunId)
  const availableModels = usePairStore((s) => s.availableModels)
  const updatePairModels = usePairStore((s) => s.updatePairModels)

  const [diffModalFile, setDiffModalFile] = useState<{ path: string; status: string } | null>(null)
  const [diffContent, setDiffContent] = useState<string | null>(null)
  const [diffLoading, setDiffLoading] = useState(false)
  const [diffError, setDiffError] = useState<string | null>(null)

  const effectiveMentorModel = pair.pendingMentorModel ?? pair.mentorModel
  const effectiveExecutorModel = pair.pendingExecutorModel ?? pair.executorModel
  const [panelMentorModel, setPanelMentorModel] = useState(effectiveMentorModel)
  const [panelExecutorModel, setPanelExecutorModel] = useState(effectiveExecutorModel)
  const [panelMentorEffort, setPanelMentorEffort] = useState<string | undefined>(
    pair.mentorReasoningEffort
  )
  const [panelExecutorEffort, setPanelExecutorEffort] = useState<string | undefined>(
    pair.executorReasoningEffort
  )
  const [saveStatus, setSaveStatus] = useState<'idle' | 'saving' | 'success' | 'error'>('idle')
  const saveStatusRef = useRef(saveStatus)
  saveStatusRef.current = saveStatus

  const modelsChanged =
    panelMentorModel !== effectiveMentorModel ||
    panelExecutorModel !== effectiveExecutorModel ||
    panelMentorEffort !== pair.mentorReasoningEffort ||
    panelExecutorEffort !== pair.executorReasoningEffort

  const handleSaveModels = async (): Promise<void> => {
    setSaveStatus('saving')
    try {
      await updatePairModels(pair.id, {
        mentorModel: panelMentorModel,
        executorModel: panelExecutorModel,
        mentorReasoningEffort: panelMentorEffort,
        executorReasoningEffort: panelExecutorEffort
      })
      setSaveStatus('success')
    } catch {
      setSaveStatus('error')
    }
  }

  useEffect(() => {
    if (saveStatus === 'success' || saveStatus === 'error') {
      const timer = setTimeout(() => {
        if (saveStatusRef.current === 'success' || saveStatusRef.current === 'error') {
          setSaveStatus('idle')
        }
      }, 2500)
      return () => clearTimeout(timer)
    }
  }, [saveStatus])

  const viewingRun = viewingRunId
    ? (pair.runHistory.find((run) => run.id === viewingRunId) ?? null)
    : null

  const timelineData = React.useMemo(() => {
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
        messages,
        latestAcceptance: source.latestAcceptance,
        modifiedFiles: 'modifiedFiles' in source ? source.modifiedFiles : pair.modifiedFiles,
        currentRunStartedAt:
          'currentRunStartedAt' in source ? source.currentRunStartedAt : pair.currentRunStartedAt,
        currentRunFinishedAt:
          'currentRunFinishedAt' in source ? source.currentRunFinishedAt : pair.currentRunFinishedAt
      }
    )
  }, [pair, viewingRun])

  const reviewReason =
    pair.status === 'Paused' || pair.status === 'Awaiting Human Review'
      ? (pair.pauseMessage ??
        pair.mentorActivity.detail ??
        pair.executorActivity.detail ??
        pair.currentTurnCard?.content ??
        (pair.status === 'Awaiting Human Review' ? t('pair.humanReview') : t('common.paused')))
      : null

  const canPause = isPairActive(pair.status)
  const handleRetryTurn = (): void => {
    void retryTurn(pair.id)
  }

  const handleFileClick = async (file: { path: string; status: string }): Promise<void> => {
    setDiffModalFile(file)
    setDiffContent(null)
    setDiffLoading(true)
    setDiffError(null)
    try {
      const diff = await window.api.repo.getFileDiff(pair.directory, file.path, file.status)
      setDiffContent(diff)
    } catch (err) {
      setDiffError(err instanceof Error ? err.message : t('pair.failedToLoadDiff'))
    } finally {
      setDiffLoading(false)
    }
  }

  const formatRunStamp = (ts?: number): string => {
    if (!ts) return t('pair.stillActive')
    const d = new Date(ts)
    return `${d.getMonth() + 1}/${d.getDate()} ${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`
  }

  const runStateText =
    pair.status === 'Paused'
      ? `${t('common.paused')} ${formatRunStamp(pair.currentRunFinishedAt)}`
      : pair.currentRunFinishedAt
        ? `${t('pair.finishedAt', { date: formatRunStamp(pair.currentRunFinishedAt) })}`
        : t('pair.stillRunning')

  const mentorIsExecuting = isAgentExecuting(pair.mentorActivity.phase)
  const executorIsExecuting = isAgentExecuting(pair.executorActivity.phase)

  return (
    <aside
      className={cn(
        'flex h-full flex-col gap-3 overflow-y-auto border-l border-border bg-background/40 px-4 py-3 font-mono text-[11px] scrollbar-thin',
        className
      )}
    >
      <div className="flex flex-wrap items-center gap-1.5">
        {pair.status === 'Paused' && (
          <GlassButton
            variant="secondary"
            size="sm"
            icon={<Play size={11} />}
            onClick={() => void onResume()}
            disabled={isStoreBusy}
          >
            {t('pair.resumePair')}
            <span className="ml-1 text-[9px] opacity-60">
              {modifierLabel}
              {shiftLabel}P
            </span>
          </GlassButton>
        )}
        {canPause && (
          <GlassButton
            variant="secondary"
            size="sm"
            icon={<Pause size={11} />}
            onClick={() => void onPause()}
            disabled={!canPause || isStoreBusy}
          >
            {t('pair.pausePair')}
            <span className="ml-1 text-[9px] opacity-60">{modifierLabel}P</span>
          </GlassButton>
        )}
        {pair.status === 'Error' && (
          <GlassButton
            variant="primary"
            size="sm"
            icon={<RotateCcw size={11} />}
            onClick={handleRetryTurn}
          >
            {t('pair.retryTurn')}
          </GlassButton>
        )}
        {pair.automationMode === 'full-auto' && (
          <span className="inline-flex items-center gap-1 border border-state-running/40 bg-state-running/12 px-1.5 py-px text-[9px] uppercase tracking-[0.14em] state-running">
            <Zap size={10} />
            {t('pair.fullAuto')}
          </span>
        )}
      </div>

      {(pair.status === 'Paused' || pair.status === 'Awaiting Human Review') && reviewReason && (
        <div className="border-l-2 border-state-running pl-2 text-[11px] leading-relaxed">
          <div className="mb-0.5 text-[10px] uppercase tracking-[0.16em] state-running">
            ! {t('pair.pauseReason')}
          </div>
          <div className="whitespace-pre-wrap text-foreground/80 [overflow-wrap:anywhere]">
            {reviewReason}
          </div>
        </div>
      )}

      {pair.status === 'Error' && (
        <ErrorDetailPanel
          error={
            (pair.mentorActivity.phase === 'error' ? pair.mentorActivity.detail : null) ??
            (pair.executorActivity.phase === 'error' ? pair.executorActivity.detail : null) ??
            t('errors.agentError')
          }
          onRetry={handleRetryTurn}
        />
      )}

      {pair.latestAcceptance && (
        <div className="space-y-1">
          <SectionHeader label={t('pair.latestAcceptance')} />
          <div className="flex items-center gap-1.5">
            {pair.latestAcceptance.verdict && (
              <span
                className={cn(
                  'border px-1.5 py-px text-[9px] uppercase tracking-[0.14em]',
                  pair.latestAcceptance.verdict.verdict === 'pass'
                    ? 'border-state-done bg-state-done state-done'
                    : 'border-state-running bg-state-running state-running'
                )}
              >
                [{pair.latestAcceptance.verdict.verdict}]
              </span>
            )}
            <span className="text-muted-foreground">risk·{pair.latestAcceptance.risk}</span>
            <span className="text-muted-foreground">
              fail·
              {pair.latestAcceptance.checks.filter((c) => c.status === 'failed').length}
            </span>
            {pair.latestAcceptance.error && <span className="state-error">err</span>}
          </div>
          <p className="text-foreground/80 leading-relaxed [overflow-wrap:anywhere]">
            {pair.latestAcceptance.summary}
          </p>
        </div>
      )}

      <div className="space-y-1">
        <SectionHeader label={t('common.models')} />
        <div className="space-y-2">
          <div className="flex items-baseline justify-between">
            <span className="flex items-baseline gap-1.5">
              <span aria-hidden className={cn('role-mentor', mentorIsExecuting && 'tty-blink')}>
                {mentorIsExecuting ? '*' : '●'}
              </span>
              <span className="role-mentor font-bold tracking-[0.08em]">
                {t('common.mentor').toUpperCase()}
              </span>
              <span className="text-[9px] uppercase tracking-[0.14em] text-muted-foreground-faint">
                · {t('panel.modelsActiveBadge')}
              </span>
            </span>
            {mentorIsExecuting && (
              <span className="text-[9px] uppercase tracking-[0.14em] state-running">
                {t('common.active')}
              </span>
            )}
          </div>
          <ModelPicker
            value={panelMentorModel}
            models={availableModels}
            onChange={setPanelMentorModel}
            role="mentor"
            variant="inline"
            reasoningEffort={panelMentorEffort}
            onReasoningEffortChange={setPanelMentorEffort}
          />
          {pair.pendingMentorModel && (
            <div className="text-[10px] state-running flex items-baseline gap-1">
              <span className="uppercase tracking-[0.14em] text-[9px]">
                · {t('panel.modelsQueuedBadge')}
              </span>
              <span className="text-foreground/80">
                → {t('pair.nextTask', { model: pair.pendingMentorModel })}
              </span>
            </div>
          )}
          <div className="flex items-baseline justify-between mt-2">
            <span className="flex items-baseline gap-1.5">
              <span aria-hidden className={cn('role-executor', executorIsExecuting && 'tty-blink')}>
                {executorIsExecuting ? '*' : '●'}
              </span>
              <span className="role-executor font-bold tracking-[0.08em]">
                {t('common.executor').toUpperCase()}
              </span>
              <span className="text-[9px] uppercase tracking-[0.14em] text-muted-foreground-faint">
                · {t('panel.modelsActiveBadge')}
              </span>
            </span>
            {executorIsExecuting && (
              <span className="text-[9px] uppercase tracking-[0.14em] state-running">
                {t('common.active')}
              </span>
            )}
          </div>
          <ModelPicker
            value={panelExecutorModel}
            models={availableModels}
            onChange={setPanelExecutorModel}
            role="executor"
            variant="inline"
            reasoningEffort={panelExecutorEffort}
            onReasoningEffortChange={setPanelExecutorEffort}
          />
          {pair.pendingExecutorModel && (
            <div className="text-[10px] state-running flex items-baseline gap-1">
              <span className="uppercase tracking-[0.14em] text-[9px]">
                · {t('panel.modelsQueuedBadge')}
              </span>
              <span className="text-foreground/80">
                → {t('pair.nextTask', { model: pair.pendingExecutorModel })}
              </span>
            </div>
          )}
          {(modelsChanged || saveStatus === 'success' || saveStatus === 'error') && (
            <div className="flex flex-col gap-1 pt-1">
              <div className="flex items-center gap-2 min-h-[26px]">
                {saveStatus === 'success' ? (
                  <span className="flex items-center gap-1 text-[10px] font-semibold state-done animate-in fade-in">
                    <Check size={10} />
                    {isPairActive(pair.status) ? t('panel.savedQueued') : t('panel.saved')}
                  </span>
                ) : saveStatus === 'error' ? (
                  <span className="flex items-center gap-1 text-[10px] font-semibold state-error animate-in fade-in">
                    ✗ {t('panel.saveFailed')}
                  </span>
                ) : (
                  <GlassButton
                    variant="primary"
                    size="sm"
                    onClick={handleSaveModels}
                    disabled={isStoreBusy || saveStatus !== 'idle'}
                  >
                    {saveStatus === 'saving' ? (
                      <>
                        <Loader2 size={10} className="animate-spin" />
                        {t('common.saving')}
                      </>
                    ) : (
                      <>
                        <SlidersHorizontal size={10} />
                        {isPairActive(pair.status)
                          ? t('panel.saveActionRunning')
                          : t('panel.saveActionIdle')}
                      </>
                    )}
                  </GlassButton>
                )}
              </div>
              {saveStatus === 'idle' && (
                <span className="text-[10px] text-muted-foreground-faint leading-snug [overflow-wrap:anywhere]">
                  {isPairActive(pair.status)
                    ? t('panel.modelsUpdateHintRunning')
                    : t('panel.modelsUpdateHintIdle')}
                </span>
              )}
            </div>
          )}
        </div>
      </div>

      <div className="space-y-1.5">
        <SectionHeader label={t('pair.runState')} />
        <div className="flex items-baseline justify-between">
          <span className="text-muted-foreground">{t('pair.run', { count: pair.runCount })}</span>
          <span className="border border-border bg-secondary/40 px-1.5 py-px text-[10px] uppercase tracking-[0.14em] text-foreground/90">
            {t(`status.${pair.status}`, { defaultValue: pair.status })}
          </span>
        </div>
        <div className="text-[10px] text-muted-foreground">
          {t('pair.started', { date: formatRunStamp(pair.currentRunStartedAt) })} · {runStateText}
        </div>
        <IterationProgress
          current={pair.iterations}
          max={pair.maxIterations}
          adaptiveBudget={pair.adaptiveBudget}
        />
      </div>

      <div className="space-y-1.5">
        <SectionHeader label={t('pair.systemResources')} />
        <ResourceMeter cpu={pair.cpuUsage} mem={pair.memUsage} compact />
      </div>

      <div className="space-y-1">
        <SectionHeader label={t('pair.modifiedFiles')} />
        {!pair.gitTracking.available ? (
          <div className="text-[10px] state-running">! {t('pair.gitUnavailable')}</div>
        ) : pair.modifiedFiles.length === 0 ? (
          <div className="text-[10px] text-muted-foreground-faint">
            — {t('pair.noModifiedFiles')}
          </div>
        ) : (
          <div className="space-y-px">
            {pair.modifiedFiles.map((file, index) => (
              <button
                key={index}
                onClick={() => handleFileClick(file)}
                title={file.path}
                className="flex w-full items-baseline gap-2 truncate text-left text-[10px] text-muted-foreground hover:bg-foreground/[0.05] px-1 -mx-1 rounded-sm transition-colors"
              >
                <span
                  className={cn(
                    'shrink-0 tabular-nums w-[1ch]',
                    file.status === 'A' && 'state-done',
                    file.status === 'M' && 'state-running',
                    file.status === 'D' && 'state-error',
                    file.status === 'R' && 'role-executor',
                    file.status === '??' && 'text-muted-foreground-faint'
                  )}
                >
                  {file.status === '??' ? '?' : file.status}
                </span>
                <span className="truncate">{file.displayPath}</span>
              </button>
            ))}
          </div>
        )}
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

      <FileDiffModal
        isOpen={diffModalFile !== null}
        onClose={() => setDiffModalFile(null)}
        filePath={diffModalFile?.path ?? ''}
        status={diffModalFile?.status ?? ''}
        diff={diffContent}
        loading={diffLoading}
        error={diffError}
      />
    </aside>
  )
}

export default PairOperationsPanel
