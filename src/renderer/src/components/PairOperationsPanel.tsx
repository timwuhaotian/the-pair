import React, { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Check,
  ChevronDown,
  CloudUpload,
  GitCommitHorizontal,
  Loader2,
  Pause,
  Play,
  SlidersHorizontal,
  Zap
} from 'lucide-react'
import { cn, extractErrorMessage } from '../lib/utils'
import { fieldInput, hoverTint } from '../lib/ui'
import { verdictTone } from '../lib/acceptanceTone'
import { usePairStore, type Pair } from '../store/usePairStore'
import { TaskHistoryPanel } from './TaskHistoryPanel'
import { TimelinePanel, TimelineActions } from './TimelinePanel'
import { ErrorDetailPanel } from './ErrorDetailPanel'
import { IterationProgress } from './IterationProgress'
import { GlassButton } from './ui/GlassButton'
import { Chip } from './ui/Chip'
import { ResourceMeter } from './ui/ResourceMeter'
import { buildTimeline } from '../lib/timeline'
import { isPairActive, isPairBusy } from '../lib/pairStatus'
import { isAgentExecuting } from '../lib/helpers'
import { modifierLabel, shiftLabel } from '../lib/shortcuts'
import { ModelPicker } from './ModelPicker'
import { InspectorSection } from './inspector/InspectorSection'
import { InlineDiff } from './inspector/InlineDiff'
import { buildTimelineSource, resolveViewingRun } from './consoleView'

interface PairOperationsPanelProps {
  pair: Pair
  onPause: () => Promise<void>
  onResume: () => Promise<void>
  onRestoreTask: (spec: string, mentorModel: string, executorModel: string) => void
  className?: string
}

function fileStatusTone(status: string): string {
  if (status === 'A') return 'state-done'
  if (status === 'M') return 'state-running'
  if (status === 'D') return 'state-error'
  if (status === 'R') return 'role-executor'
  return 'text-muted-foreground-faint'
}

interface ModifiedFile {
  path: string
  displayPath: string
  status: string
}

function ModifiedFileRow({
  file,
  expanded,
  diff,
  loading,
  error,
  onToggle
}: {
  file: ModifiedFile
  expanded: boolean
  diff: string | null
  loading: boolean
  error: string | null
  onToggle: () => void
}): React.ReactNode {
  return (
    <div>
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={expanded}
        data-testid={`ops-file-row-${file.path}`}
        title={file.path}
        className={cn(
          'flex w-full cursor-pointer items-baseline gap-2 px-2 py-1.5 text-left font-mono text-[11px] text-muted-foreground transition-colors',
          hoverTint
        )}
      >
        <ChevronDown
          size={10}
          aria-hidden
          className={cn(
            'shrink-0 translate-y-px text-muted-foreground-faint transition-transform duration-150',
            !expanded && '-rotate-90'
          )}
        />
        <span className={cn('shrink-0 tabular-nums w-[1ch]', fileStatusTone(file.status))}>
          {file.status === '??' ? '?' : file.status}
        </span>
        <span className="min-w-0 flex-1 truncate">{file.displayPath}</span>
      </button>
      {expanded && (
        <div
          data-testid={`ops-file-diff-${file.path}`}
          className="border-t border-border/40 bg-background/40 max-h-[320px] overflow-auto scrollbar-thin"
        >
          <InlineDiff diff={diff} loading={loading} error={error} />
        </div>
      )}
    </div>
  )
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

  // Expanded file with its cached diff. One open at a time keeps the inspector
  // compact; the diff cache survives toggling so re-opening is instant.
  const [expandedPath, setExpandedPath] = useState<string | null>(null)
  const [diffCache, setDiffCache] = useState<Record<string, string>>({})
  const [diffErrorCache, setDiffErrorCache] = useState<Record<string, string>>({})
  const [diffLoadingPath, setDiffLoadingPath] = useState<string | null>(null)
  // Monotonic token so a slow diff for an earlier file can't render under a later file.
  const diffRequestRef = useRef(0)
  const [isRetrying, setIsRetrying] = useState(false)
  const [retryError, setRetryError] = useState<string | null>(null)

  // Commit + push state for the Modified Files section. Statuses auto-clear
  // like saveStatus so a stale "committed abc123" doesn't linger next run.
  const [commitMessage, setCommitMessage] = useState('')
  const [commitStatus, setCommitStatus] = useState<'idle' | 'committing' | 'success' | 'error'>(
    'idle'
  )
  const [commitDetail, setCommitDetail] = useState<string | null>(null)
  const [pushStatus, setPushStatus] = useState<'idle' | 'pushing' | 'success' | 'error'>('idle')
  const [pushDetail, setPushDetail] = useState<string | null>(null)
  const commitStatusRef = useRef(commitStatus)
  commitStatusRef.current = commitStatus
  const pushStatusRef = useRef(pushStatus)
  pushStatusRef.current = pushStatus

  const pushBranch = pair.worktreeBranch ?? pair.branch ?? null
  const hasChanges = pair.gitTracking.available && pair.modifiedFiles.length > 0

  const handleCommit = async (): Promise<void> => {
    if (commitStatus === 'committing' || !hasChanges) return
    const message = commitMessage.trim()
    if (!message) return
    setCommitStatus('committing')
    setCommitDetail(null)
    try {
      const outcome = await window.api.repo.commitChanges(pair.directory, message)
      setCommitDetail(t('pair.committed', { sha: outcome.sha }))
      setCommitStatus('success')
      setCommitMessage('')
    } catch (err) {
      setCommitDetail(extractErrorMessage(err, t('pair.commitFailed')))
      setCommitStatus('error')
    }
  }

  const handlePush = async (): Promise<void> => {
    if (pushStatus === 'pushing' || !pushBranch) return
    setPushStatus('pushing')
    setPushDetail(null)
    try {
      const outcome = await window.api.repo.pushChanges(pair.directory)
      setPushDetail(
        outcome.upToDate
          ? t('pair.pushedUpToDate', { branch: outcome.branch })
          : t('pair.pushed', { branch: outcome.branch })
      )
      setPushStatus('success')
    } catch (err) {
      setPushDetail(extractErrorMessage(err, t('pair.pushFailed')))
      setPushStatus('error')
    }
  }

  useEffect(() => {
    if (commitStatus !== 'success' && commitStatus !== 'error') return
    const timer = setTimeout(() => {
      if (commitStatusRef.current === 'success' || commitStatusRef.current === 'error') {
        setCommitStatus('idle')
        setCommitDetail(null)
      }
    }, 4000)
    return () => clearTimeout(timer)
  }, [commitStatus])

  useEffect(() => {
    if (pushStatus !== 'success' && pushStatus !== 'error') return
    const timer = setTimeout(() => {
      if (pushStatusRef.current === 'success' || pushStatusRef.current === 'error') {
        setPushStatus('idle')
        setPushDetail(null)
      }
    }, 4000)
    return () => clearTimeout(timer)
  }, [pushStatus])

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

  // Re-sync local panel state when the pair's effective models change externally
  // (e.g., from another component or after a successful server-side update).
  useEffect(() => {
    setPanelMentorModel(effectiveMentorModel)
    setPanelExecutorModel(effectiveExecutorModel)
    setPanelMentorEffort(pair.mentorReasoningEffort)
    setPanelExecutorEffort(pair.executorReasoningEffort)
  }, [
    effectiveMentorModel,
    effectiveExecutorModel,
    pair.mentorReasoningEffort,
    pair.executorReasoningEffort
  ])

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

  // An id that isn't in this pair's history is treated as "live".
  const viewingRun = resolveViewingRun(pair.runHistory, viewingRunId)

  const timelineData = React.useMemo(() => {
    // Archived runs map their own dates/status explicitly — never the live run's.
    const source = buildTimelineSource(pair, viewingRun)
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
      source
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
  const handleRetryTurn = async (): Promise<void> => {
    if (isRetrying) return
    setIsRetrying(true)
    setRetryError(null)
    try {
      await retryTurn(pair.id)
    } catch (error) {
      setRetryError(extractErrorMessage(error, t('pair.retryFailed')))
    } finally {
      setIsRetrying(false)
    }
  }

  const handleToggleFile = (file: ModifiedFile): void => {
    if (expandedPath === file.path) {
      setExpandedPath(null)
      return
    }
    setExpandedPath(file.path)
    if (diffCache[file.path] || diffErrorCache[file.path] || diffLoadingPath === file.path) return
    const requestId = ++diffRequestRef.current
    setDiffLoadingPath(file.path)
    void window.api.repo
      .getFileDiff(pair.directory, file.path, file.status)
      .then((diff) => {
        if (requestId !== diffRequestRef.current) return
        setDiffCache((prev) => ({ ...prev, [file.path]: diff }))
        setDiffErrorCache((prev) => {
          const next = { ...prev }
          delete next[file.path]
          return next
        })
      })
      .catch((err: unknown) => {
        if (requestId !== diffRequestRef.current) return
        setDiffErrorCache((prev) => ({
          ...prev,
          [file.path]: err instanceof Error ? err.message : t('pair.failedToLoadDiff')
        }))
      })
      .finally(() => {
        if (requestId === diffRequestRef.current) setDiffLoadingPath(null)
      })
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
  const failedChecks = pair.latestAcceptance
    ? pair.latestAcceptance.checks.filter((c) => c.status === 'failed').length
    : 0

  return (
    <aside
      className={cn(
        'flex h-full flex-col gap-4 overflow-y-auto border-l border-border bg-background px-4 py-4 text-[13px] scrollbar-thin',
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
            data-testid="ops-resume-btn"
          >
            {t('pair.resumePair')}
            <span className="ml-1 text-[10px] opacity-60">
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
            data-testid="ops-pause-btn"
          >
            {t('pair.pausePair')}
            <span className="ml-1 text-[10px] opacity-60">{modifierLabel}P</span>
          </GlassButton>
        )}
        {pair.automationMode === 'full-auto' && (
          <Chip tone="running">
            <Zap size={10} aria-hidden />
            {t('pair.fullAuto')}
          </Chip>
        )}
      </div>

      {(pair.status === 'Paused' || pair.status === 'Awaiting Human Review') && reviewReason && (
        <div className="border-l-2 border-state-running pl-2 text-[11px] leading-relaxed">
          <div className="mb-0.5 text-[10px] uppercase tracking-[0.14em] text-muted-foreground">
            {t('pair.pauseReason')}
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
          onRetry={() => void handleRetryTurn()}
          isRetrying={isRetrying}
        />
      )}

      {retryError && pair.status === 'Error' && (
        <div
          role="alert"
          data-testid="ops-retry-error"
          className="border-l-2 border-state-error pl-2 text-[11px] leading-relaxed state-error [overflow-wrap:anywhere]"
        >
          ✗ {retryError}
        </div>
      )}

      {pair.latestAcceptance && (
        <InspectorSection
          id="acceptance"
          label={t('pair.latestAcceptance')}
          defaultOpen={false}
          badge={
            pair.latestAcceptance.verdict && (
              <Chip tone={verdictTone(pair.latestAcceptance.verdict.verdict)}>
                {pair.latestAcceptance.verdict.verdict}
              </Chip>
            )
          }
        >
          <div className="flex flex-wrap items-center gap-1.5">
            <Chip>risk·{pair.latestAcceptance.risk}</Chip>
            <Chip tone={failedChecks > 0 ? 'error' : 'neutral'}>fail·{failedChecks}</Chip>
            {pair.latestAcceptance.error && <Chip tone="error">{t('pair.verdictError')}</Chip>}
          </div>
          <p className="text-foreground/80 leading-relaxed [overflow-wrap:anywhere]">
            {pair.latestAcceptance.summary}
          </p>
        </InspectorSection>
      )}

      <InspectorSection
        id="models"
        label={t('common.models')}
        badge={
          (pair.pendingMentorModel || pair.pendingExecutorModel) && (
            <Chip tone="running">{t('panel.modelsQueuedBadge')}</Chip>
          )
        }
      >
        <div className="space-y-2">
          <div className="flex items-center justify-between gap-2">
            <span className="flex items-center gap-1.5">
              <Chip tone="mentor">{t('common.mentor')}</Chip>
              <span className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground-faint">
                {t('panel.modelsActiveBadge')}
              </span>
            </span>
            {mentorIsExecuting && <Chip tone="running">{t('common.active')}</Chip>}
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
            <div className="flex items-center gap-1.5 text-[11px]">
              <Chip tone="running">{t('panel.modelsQueuedBadge')}</Chip>
              <span className="min-w-0 truncate text-foreground/80">
                {t('pair.nextTask', { model: pair.pendingMentorModel })}
              </span>
            </div>
          )}
          <div className="flex items-center justify-between gap-2">
            <span className="flex items-center gap-1.5">
              <Chip tone="executor">{t('common.executor')}</Chip>
              <span className="text-[10px] uppercase tracking-[0.14em] text-muted-foreground-faint">
                {t('panel.modelsActiveBadge')}
              </span>
            </span>
            {executorIsExecuting && <Chip tone="running">{t('common.active')}</Chip>}
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
            <div className="flex items-center gap-1.5 text-[11px]">
              <Chip tone="running">{t('panel.modelsQueuedBadge')}</Chip>
              <span className="min-w-0 truncate text-foreground/80">
                {t('pair.nextTask', { model: pair.pendingExecutorModel })}
              </span>
            </div>
          )}
          {(modelsChanged || saveStatus === 'success' || saveStatus === 'error') && (
            <div className="flex flex-col gap-1">
              <div className="flex min-h-[28px] items-center gap-2">
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
                    icon={
                      saveStatus === 'saving' ? (
                        <Loader2 size={11} className="animate-spin" />
                      ) : (
                        <SlidersHorizontal size={11} />
                      )
                    }
                  >
                    {saveStatus === 'saving'
                      ? t('common.saving')
                      : isPairActive(pair.status)
                        ? t('panel.saveActionRunning')
                        : t('panel.saveActionIdle')}
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
      </InspectorSection>

      <InspectorSection
        id="run-state"
        label={t('pair.runState')}
        badge={<Chip>{t(`status.${pair.status}`, { defaultValue: pair.status })}</Chip>}
      >
        <div className="flex items-baseline justify-between">
          <span className="text-muted-foreground">{t('pair.run', { count: pair.runCount })}</span>
        </div>
        <div className="text-[10px] text-muted-foreground">
          {t('pair.started', { date: formatRunStamp(pair.currentRunStartedAt) })} · {runStateText}
        </div>
        <IterationProgress
          current={pair.iterations}
          max={pair.maxIterations}
          adaptiveBudget={pair.adaptiveBudget}
        />
      </InspectorSection>

      <InspectorSection id="resources" label={t('pair.systemResources')} defaultOpen={false}>
        <ResourceMeter cpu={pair.cpuUsage} mem={pair.memUsage} compact />
      </InspectorSection>

      <InspectorSection
        id="files"
        label={t('pair.modifiedFiles')}
        badge={
          hasChanges ? (
            <span className="shrink-0 tabular-nums text-[10px] text-muted-foreground">
              {pair.modifiedFiles.length}
            </span>
          ) : undefined
        }
      >
        {!pair.gitTracking.available ? (
          <div className="text-[11px] state-running">{t('pair.gitUnavailable')}</div>
        ) : (
          <>
            {pair.modifiedFiles.length === 0 ? (
              <div className="text-[11px] text-muted-foreground-faint">
                {t('pair.noModifiedFiles')}
              </div>
            ) : (
              <div className="shell-card divide-y divide-border/40 overflow-hidden">
                {pair.modifiedFiles.map((file) => (
                  <ModifiedFileRow
                    key={file.path}
                    file={file}
                    expanded={expandedPath === file.path}
                    diff={diffCache[file.path] ?? null}
                    loading={diffLoadingPath === file.path}
                    error={diffErrorCache[file.path] ?? null}
                    onToggle={() => handleToggleFile(file)}
                  />
                ))}
              </div>
            )}
            <div className="flex flex-col gap-1.5">
              <div className="flex gap-1.5">
                <input
                  value={commitMessage}
                  onChange={(e) => setCommitMessage(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') void handleCommit()
                  }}
                  placeholder={t('pair.commitPlaceholder')}
                  aria-label={t('pair.commitMessage')}
                  disabled={!hasChanges || commitStatus === 'committing'}
                  data-testid="ops-commit-input"
                  className={cn(
                    fieldInput,
                    'min-h-[28px] min-w-0 flex-1 py-1 font-mono text-[11px]'
                  )}
                />
                <GlassButton
                  variant="secondary"
                  size="sm"
                  icon={
                    commitStatus === 'committing' ? (
                      <Loader2 size={11} className="animate-spin" />
                    ) : (
                      <GitCommitHorizontal size={11} />
                    )
                  }
                  onClick={() => void handleCommit()}
                  disabled={!hasChanges || !commitMessage.trim() || commitStatus === 'committing'}
                  title={t('pair.commitActionHint')}
                  data-testid="ops-commit-btn"
                >
                  {t('pair.commit')}
                </GlassButton>
              </div>
              {commitDetail && (commitStatus === 'success' || commitStatus === 'error') && (
                <div
                  role={commitStatus === 'error' ? 'alert' : 'status'}
                  data-testid="ops-commit-detail"
                  className={cn(
                    'text-[10px] leading-relaxed [overflow-wrap:anywhere]',
                    commitStatus === 'success' ? 'state-done' : 'state-error'
                  )}
                >
                  {commitStatus === 'success' ? `✓ ${commitDetail}` : `✗ ${commitDetail}`}
                </div>
              )}
              {pushBranch && (
                <>
                  <GlassButton
                    variant="secondary"
                    size="sm"
                    icon={
                      pushStatus === 'pushing' ? (
                        <Loader2 size={11} className="animate-spin" />
                      ) : (
                        <CloudUpload size={11} />
                      )
                    }
                    onClick={() => void handlePush()}
                    disabled={pushStatus === 'pushing'}
                    title={t('pair.pushActionHint', { branch: pushBranch })}
                    data-testid="ops-push-btn"
                    className="self-start"
                  >
                    {t('pair.pushTo', { branch: pushBranch })}
                  </GlassButton>
                  {pushDetail && (pushStatus === 'success' || pushStatus === 'error') && (
                    <div
                      role={pushStatus === 'error' ? 'alert' : 'status'}
                      data-testid="ops-push-detail"
                      className={cn(
                        'text-[10px] leading-relaxed [overflow-wrap:anywhere]',
                        pushStatus === 'success' ? 'state-done' : 'state-error'
                      )}
                    >
                      {pushStatus === 'success' ? `✓ ${pushDetail}` : `✗ ${pushDetail}`}
                    </div>
                  )}
                </>
              )}
            </div>
          </>
        )}
      </InspectorSection>

      {/* Restoring starts a new run, which would kill the one in progress. */}
      <InspectorSection
        id="history"
        label={t('pair.taskHistory')}
        defaultOpen={pair.runHistory.length > 0}
        badge={
          pair.runHistory.length > 0 ? (
            <span className="shrink-0 tabular-nums text-[10px] text-muted-foreground">
              {pair.runHistory.length}
            </span>
          ) : undefined
        }
      >
        <TaskHistoryPanel
          runHistory={pair.runHistory}
          viewingRunId={viewingRun?.id ?? null}
          onSelectTask={(runId) => setViewingRunId(runId)}
          onBackToCurrent={() => setViewingRunId(null)}
          onRestoreTask={(run) => onRestoreTask(run.spec, run.mentorModel, run.executorModel)}
          restoreDisabled={isPairBusy(pair.status)}
          bare
        />
      </InspectorSection>

      <InspectorSection
        id="timeline"
        label={t('pair.timeline')}
        defaultOpen={false}
        actions={timelineData ? <TimelineActions timeline={timelineData} /> : undefined}
      >
        <TimelinePanel timeline={timelineData} bare />
      </InspectorSection>
    </aside>
  )
}

export default PairOperationsPanel
