import React, { useState } from 'react'
import { AlertTriangle, Clock3, FolderOpen, RotateCcw, Sparkles, Trash2, Zap } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import type { RecoverableSessionSummary } from '../types'
import { StatusBadge } from './StatusBadge'
import { GlassButton } from './ui/GlassButton'
import { GlassModal } from './ui/GlassModal'

interface SessionRecoveryModalProps {
  sessions: RecoverableSessionSummary[]
  isOpen: boolean
  isRestoring: boolean
  deletingPairId: string | null
  onRestore: (pairId: string, continueRun: boolean) => void | Promise<void>
  onDelete: (pairId: string) => void | Promise<void>
  onDismiss: () => void
}

function formatTime(ts: number): string {
  return new Date(ts).toLocaleString()
}

function DeleteConfirmationModal({
  session,
  isOpen,
  onConfirm,
  onCancel
}: {
  session: RecoverableSessionSummary
  isOpen: boolean
  onConfirm: () => void
  onCancel: () => void
}): React.ReactNode {
  const { t } = useTranslation()
  return (
    <GlassModal
      isOpen={isOpen}
      onClose={onCancel}
      title={t('modals.confirmDelete')}
      className="max-w-md"
    >
      <div className="space-y-4">
        <div className="flex items-start gap-3 rounded-xl border border-destructive/20 bg-destructive/5 p-4">
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-destructive/10">
            <AlertTriangle className="h-5 w-5 text-destructive" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="font-medium text-foreground">{t('modals.deleteSession')}</p>
            <p className="mt-1 text-sm text-muted-foreground">{t('modals.deleteSessionDesc')}</p>
          </div>
        </div>

        <div className="space-y-2 text-sm text-muted-foreground">
          <div className="flex items-center gap-2">
            <FolderOpen size={14} className="shrink-0" />
            <span className="truncate">{session.directory}</span>
          </div>
          <div className="flex items-center gap-2">
            <Clock3 size={14} className="shrink-0" />
            <span>{formatTime(session.savedAt)}</span>
          </div>
        </div>

        <div className="flex justify-end gap-2 pt-2">
          <GlassButton variant="ghost" onClick={onCancel}>
            {t('common.cancel')}
          </GlassButton>
          <GlassButton variant="destructive" onClick={onConfirm}>
            <Trash2 size={14} />
            {t('common.delete')}
          </GlassButton>
        </div>
      </div>
    </GlassModal>
  )
}

function SessionCard({
  session,
  onRestore,
  isRestoring,
  isDeleting,
  isDeletionLocked,
  onRequestDelete
}: {
  session: RecoverableSessionSummary
  onRestore: (pairId: string, continueRun: boolean) => void | Promise<void>
  isRestoring: boolean
  isDeleting: boolean
  isDeletionLocked: boolean
  onRequestDelete: (session: RecoverableSessionSummary) => void
}): React.ReactNode {
  const { t } = useTranslation()
  const isMentor = session.turn === 'mentor'

  return (
    <div
      className={cn(
        'relative overflow-hidden rounded-2xl border border-border/70 bg-background/60 p-4 shadow-sm transition-all duration-300',
        isDeleting && 'border-blue-500/25 bg-background/80'
      )}
    >
      {isDeleting && (
        <div className="absolute inset-0 z-20 overflow-hidden rounded-2xl border border-blue-500/20 bg-background/85 backdrop-blur-md">
          <div className="absolute inset-0 bg-[radial-gradient(circle_at_top_left,rgba(59,130,246,0.14),transparent_30%),radial-gradient(circle_at_top_right,rgba(168,85,247,0.12),transparent_28%),linear-gradient(135deg,rgba(255,255,255,0.04),transparent_40%)]" />
          <div className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-blue-400/70 to-transparent animate-pulse" />
          <div className="relative flex h-full min-h-[220px] flex-col justify-between p-4">
            <div className="flex items-start justify-between gap-4">
              <div className="space-y-2">
                <div className="inline-flex items-center gap-2 rounded-full border border-blue-500/20 bg-blue-500/10 px-2.5 py-1 text-[10px] font-semibold uppercase tracking-[0.22em] text-blue-700 dark:text-blue-300">
                  <Zap size={12} />
                  {t('common.deleting')}
                </div>
                <div className="space-y-1">
                  <div className="h-4 w-44 rounded-full bg-muted/40 animate-pulse" />
                  <div className="h-3 w-56 rounded-full bg-muted/25 animate-pulse" />
                </div>
              </div>
              <div className="flex h-10 w-10 items-center justify-center rounded-2xl border border-blue-500/25 bg-blue-500/10 text-blue-500 shadow-[0_0_18px_rgba(59,130,246,0.18)]">
                <span className="metal-sheen-emblem">
                  <Zap size={15} />
                </span>
              </div>
            </div>

            <div className="space-y-3">
              <div className="h-2.5 w-full rounded-full bg-muted/30">
                <div className="h-full w-1/3 rounded-full bg-gradient-to-r from-blue-500 via-cyan-400 to-amber-400 animate-[pulse_1.2s_ease-in-out_infinite]" />
              </div>
              <div className="grid gap-2">
                <div className="h-3 w-4/5 rounded-full bg-muted/20 animate-pulse" />
                <div className="h-3 w-2/3 rounded-full bg-muted/20 animate-pulse" />
                <div className="h-3 w-1/2 rounded-full bg-muted/20 animate-pulse" />
              </div>
            </div>

            <div className="flex items-center justify-between text-[11px] text-muted-foreground">
              <span>{t('common.deleting')}</span>
              <span className="font-mono uppercase tracking-[0.18em] text-blue-600 dark:text-blue-300">
                WAIT
              </span>
            </div>
          </div>
        </div>
      )}
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0 space-y-2">
          <div className="flex items-center gap-2">
            <h3 className="truncate text-base font-semibold text-foreground">{session.name}</h3>
            <StatusBadge status={session.status} />
          </div>
          <div className="flex flex-wrap items-center gap-2 text-[11px] text-muted-foreground">
            <span className="inline-flex items-center gap-1 rounded-full border border-border/60 bg-muted/30 px-2 py-1">
              <FolderOpen size={11} />
              <span className="truncate max-w-[240px]">{session.directory}</span>
            </span>
            <span className="inline-flex items-center gap-1 rounded-full border border-border/60 bg-muted/30 px-2 py-1">
              <Clock3 size={11} />
              <span>{formatTime(session.savedAt)}</span>
            </span>
          </div>
        </div>
        <div
          className={cn(
            'flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl border',
            isMentor
              ? 'border-blue-500/25 bg-blue-500/10 text-blue-600 dark:text-blue-400'
              : 'border-purple-500/25 bg-purple-500/10 text-purple-600 dark:text-purple-400'
          )}
        >
          {isMentor ? <Sparkles size={15} /> : <Zap size={15} />}
        </div>
      </div>

      <div className="mt-4 grid gap-2 text-sm text-muted-foreground md:grid-cols-2">
        <div className="rounded-xl border border-border/60 bg-muted/20 px-3 py-2">
          <div className="text-[10px] uppercase tracking-[0.2em] text-muted-foreground">
            {t('modals.currentTurn')}
          </div>
          <div className="mt-1 font-medium text-foreground">{session.turn}</div>
        </div>
        <div className="rounded-xl border border-border/60 bg-muted/20 px-3 py-2">
          <div className="text-[10px] uppercase tracking-[0.2em] text-muted-foreground">
            {t('common.models')}
          </div>
          <div className="mt-1 truncate font-mono text-xs text-foreground">
            {session.mentorModel.split('/').pop()} / {session.executorModel.split('/').pop()}
          </div>
        </div>
      </div>

      <div className="mt-3 text-sm leading-relaxed text-muted-foreground whitespace-pre-wrap break-words [overflow-wrap:anywhere]">
        {session.currentTurnCard?.content || session.spec}
      </div>

      <div className="mt-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex flex-wrap items-center gap-2 text-[11px] text-muted-foreground">
          <span className="rounded-full border border-border/60 bg-muted/30 px-2 py-1">
            Runs {session.runCount}
          </span>
          {session.hasMentorSession && (
            <span className="rounded-full border border-blue-500/20 bg-blue-500/10 px-2 py-1 text-blue-700 dark:text-blue-300">
              {t('modals.mentorSaved')}
            </span>
          )}
          {session.hasExecutorSession && (
            <span className="rounded-full border border-purple-500/20 bg-purple-500/10 px-2 py-1 text-purple-700 dark:text-purple-300">
              {t('modals.executorSaved')}
            </span>
          )}
        </div>
        <div className="flex flex-col gap-2 sm:flex-row">
          <GlassButton
            variant="destructive"
            size="sm"
            onClick={() => onRequestDelete(session)}
            disabled={isRestoring || isDeletionLocked}
          >
            <Trash2 size={14} />
            {t('common.delete')}
          </GlassButton>
          <GlassButton
            variant="ghost"
            size="sm"
            onClick={() => void onRestore(session.pairId, false)}
            disabled={isRestoring || isDeletionLocked}
          >
            {t('modals.restoreHistory')}
          </GlassButton>
          <GlassButton
            variant="primary"
            size="sm"
            onClick={() => void onRestore(session.pairId, true)}
            disabled={isRestoring || isDeletionLocked}
            icon={<RotateCcw size={13} />}
          >
            {t('modals.resumeNewTask')}
          </GlassButton>
        </div>
      </div>
    </div>
  )
}

export function SessionRecoveryModal({
  sessions,
  isOpen,
  isRestoring,
  deletingPairId,
  onRestore,
  onDelete,
  onDismiss
}: SessionRecoveryModalProps): React.ReactNode {
  const { t } = useTranslation()
  const [pendingDeleteSession, setPendingDeleteSession] =
    useState<RecoverableSessionSummary | null>(null)

  if (!isOpen || sessions.length === 0) return null

  return (
    <>
      <GlassModal
        isOpen={isOpen}
        onClose={onDismiss}
        title={t('modals.recoverTitle')}
        className="max-w-4xl"
      >
        <div className="space-y-4">
          <p className="text-sm leading-relaxed text-muted-foreground">
            The app found session records from a previous run. Restore history to inspect the saved
            messages and state, or explicitly resume execution if you want the agents to keep going.
          </p>

          <div className="max-h-[62vh] space-y-3 overflow-y-auto pr-1 scrollbar-thin">
            {sessions.map((session) => (
              <SessionCard
                key={session.pairId}
                session={session}
                onRestore={onRestore}
                isRestoring={isRestoring}
                isDeleting={deletingPairId === session.pairId}
                isDeletionLocked={deletingPairId !== null}
                onRequestDelete={setPendingDeleteSession}
              />
            ))}
          </div>

          <div className="flex flex-col-reverse gap-3 pt-2 sm:flex-row sm:justify-between">
            <GlassButton variant="ghost" onClick={onDismiss}>
              {t('modals.startFresh')}
            </GlassButton>
            <div className="text-xs leading-relaxed text-muted-foreground sm:text-right">
              {t('modals.dismissDesc')}
            </div>
          </div>
        </div>
      </GlassModal>

      {pendingDeleteSession && (
        <DeleteConfirmationModal
          session={pendingDeleteSession}
          isOpen={pendingDeleteSession !== null}
          onConfirm={() => {
            if (pendingDeleteSession) {
              void onDelete(pendingDeleteSession.pairId)
              setPendingDeleteSession(null)
            }
          }}
          onCancel={() => setPendingDeleteSession(null)}
        />
      )}
    </>
  )
}
