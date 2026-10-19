import React from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowLeft, RotateCcw } from 'lucide-react'
import { cn } from '../lib/utils'
import { activeTint, hoverTint } from '../lib/ui'
import { verdictTone } from '../lib/acceptanceTone'
import { GlassButton } from './ui/GlassButton'
import { IconButton } from './ui/IconButton'
import { Chip } from './ui/Chip'
import { TerminalDivider } from './terminal/TerminalDivider'
import type { PairRunSummary } from '../store/usePairStore'

interface TaskHistoryPanelProps {
  runHistory: PairRunSummary[]
  viewingRunId: string | null
  onSelectTask: (runId: string) => void
  onBackToCurrent: () => void
  onRestoreTask: (run: PairRunSummary) => void
  /** Disables restore while the pair is busy — restoring would kill the running run. */
  restoreDisabled?: boolean
  /** Rendered inside an InspectorSection; when set, skip the own header. */
  bare?: boolean
}

function formatDate(ts: number): string {
  const d = new Date(ts)
  return `${d.getMonth() + 1}/${d.getDate()} ${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`
}

function getDuration(startedAt: number, finishedAt?: number): string {
  const end = finishedAt ?? Date.now()
  const seconds = Math.floor((end - startedAt) / 1000)
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`
  const hours = Math.floor(minutes / 60)
  return `${hours}h ${minutes % 60}m`
}

/** Leave an archived run. Shared by the history list and the console header. */
export function BackToCurrentButton({
  onClick,
  'data-testid': dataTestId
}: {
  onClick: () => void
  'data-testid'?: string
}): React.ReactNode {
  const { t } = useTranslation()
  return (
    <GlassButton
      variant="secondary"
      size="xs"
      icon={<ArrowLeft size={11} />}
      onClick={onClick}
      data-testid={dataTestId}
    >
      {t('history.backToCurrent')}
    </GlassButton>
  )
}

function HistoryList({
  runHistory,
  viewingRunId,
  onSelectTask,
  onBackToCurrent,
  onRestoreTask,
  restoreDisabled
}: Omit<TaskHistoryPanelProps, 'bare'>): React.ReactNode {
  const { t } = useTranslation()

  if (runHistory.length === 0) {
    return <p className="text-[11px] text-muted-foreground-faint">{t('history.empty')}</p>
  }

  const sorted = [...runHistory].reverse()

  return (
    <>
      {viewingRunId && (
        <div className="flex justify-end">
          <BackToCurrentButton onClick={onBackToCurrent} />
        </div>
      )}
      <div className="shell-card overflow-hidden">
        <div className="max-h-[320px] overflow-y-auto scrollbar-thin">
          {sorted.map((run, idx) => {
            const isViewing = viewingRunId === run.id
            const duration = getDuration(run.startedAt, run.finishedAt)
            const modelShort = (model: string): string => model.split('/').pop() ?? model

            return (
              <div
                key={run.id}
                className={cn(
                  'group relative border-b border-border/40 last:border-b-0 transition-colors duration-150',
                  isViewing ? activeTint : hoverTint
                )}
              >
                <button
                  type="button"
                  className="w-full text-left px-2 py-2 cursor-pointer"
                  onClick={() => onSelectTask(run.id)}
                >
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="text-[10px] text-muted-foreground-faint tabular-nums shrink-0">
                      #{runHistory.length - idx}
                    </span>
                    <span className="text-[10px] tabular-nums text-muted-foreground-faint shrink-0 mr-7">
                      {duration}
                    </span>
                  </div>
                  <p
                    className="mt-0.5 line-clamp-2 font-mono text-[11px] leading-relaxed text-foreground/85 group-hover:line-clamp-3"
                    title={run.spec}
                  >
                    {run.spec}
                  </p>
                  <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[10px] text-muted-foreground-faint">
                    <span className="font-mono">
                      <span className="role-mentor">{modelShort(run.mentorModel)}</span>
                      <span className="text-muted-foreground-faint"> / </span>
                      <span className="role-executor">{modelShort(run.executorModel)}</span>
                    </span>
                    {run.latestAcceptance?.verdict && (
                      <>
                        <Chip tone={verdictTone(run.latestAcceptance.verdict.verdict)}>
                          {run.latestAcceptance.verdict.verdict}
                        </Chip>
                        <Chip>{run.latestAcceptance.risk}</Chip>
                      </>
                    )}
                    <span className="ml-auto tabular-nums">{formatDate(run.startedAt)}</span>
                  </div>
                </button>

                <IconButton
                  onClick={(e) => {
                    e.stopPropagation()
                    if (restoreDisabled) return
                    onRestoreTask(run)
                  }}
                  disabled={restoreDisabled}
                  aria-label={t('history.restoreTask')}
                  title={
                    restoreDisabled ? t('history.restoreDisabledBusy') : t('history.restoreTask')
                  }
                  data-testid="history-restore-btn"
                  className={cn(
                    'absolute right-1 top-1',
                    !restoreDisabled && !isViewing && 'opacity-50 group-hover:opacity-100'
                  )}
                >
                  <RotateCcw size={12} />
                </IconButton>
              </div>
            )
          })}
        </div>
      </div>
    </>
  )
}

export function TaskHistoryPanel({
  runHistory,
  viewingRunId,
  onSelectTask,
  onBackToCurrent,
  onRestoreTask,
  restoreDisabled = false,
  bare = false
}: TaskHistoryPanelProps): React.ReactNode {
  const { t } = useTranslation()

  if (bare) {
    return (
      <HistoryList
        runHistory={runHistory}
        viewingRunId={viewingRunId}
        onSelectTask={onSelectTask}
        onBackToCurrent={onBackToCurrent}
        onRestoreTask={onRestoreTask}
        restoreDisabled={restoreDisabled}
      />
    )
  }

  return (
    <div className="space-y-1">
      <div className="flex items-baseline justify-between">
        <TerminalDivider label={t('history.title')} className="flex-1" />
      </div>
      <HistoryList
        runHistory={runHistory}
        viewingRunId={viewingRunId}
        onSelectTask={onSelectTask}
        onBackToCurrent={onBackToCurrent}
        onRestoreTask={onRestoreTask}
        restoreDisabled={restoreDisabled}
      />
    </div>
  )
}
