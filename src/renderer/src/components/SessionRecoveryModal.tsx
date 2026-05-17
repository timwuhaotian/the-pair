import React, { useState } from 'react'
import { AlertTriangle, Clock3, FolderOpen, RotateCcw, Trash2 } from 'lucide-react'
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
      <div className="space-y-3 font-mono text-[12px]">
        <div className="border-l-2 border-state-error bg-state-error/8 px-3 py-2">
          <div className="flex items-baseline gap-2">
            <AlertTriangle size={11} className="state-error translate-y-px" />
            <div>
              <p className="text-foreground/90">{t('modals.deleteSession')}</p>
              <p className="mt-0.5 text-[11px] text-muted-foreground">
                {t('modals.deleteSessionDesc')}
              </p>
            </div>
          </div>
        </div>

        <div className="space-y-1 text-[11px] text-muted-foreground">
          <div className="flex items-baseline gap-2">
            <FolderOpen size={11} className="shrink-0 translate-y-px" />
            <span className="truncate">{session.directory}</span>
          </div>
          <div className="flex items-baseline gap-2">
            <Clock3 size={11} className="shrink-0 translate-y-px" />
            <span>{formatTime(session.savedAt)}</span>
          </div>
        </div>

        <div className="flex justify-end gap-2 pt-1">
          <GlassButton variant="ghost" size="sm" onClick={onCancel}>
            {t('common.cancel')}
          </GlassButton>
          <GlassButton variant="destructive" size="sm" onClick={onConfirm}>
            <Trash2 size={11} />
            {t('common.delete')}
          </GlassButton>
        </div>
      </div>
    </GlassModal>
  )
}

function SessionRow({
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
        'relative border border-border rounded-sm bg-background/40 p-3 transition-opacity font-mono',
        isDeleting && 'opacity-50'
      )}
    >
      <div className="flex items-baseline justify-between gap-3">
        <div className="min-w-0 flex items-baseline gap-2">
          <span aria-hidden className={cn('shrink-0', isMentor ? 'role-mentor' : 'role-executor')}>
            ●
          </span>
          <h3 className="truncate text-[13px] font-bold text-foreground/90">{session.name}</h3>
          <StatusBadge status={session.status} />
        </div>
      </div>

      <div className="mt-1.5 flex flex-wrap items-baseline gap-x-3 gap-y-0.5 text-[10px] text-muted-foreground">
        <span className="inline-flex items-baseline gap-1">
          <FolderOpen size={10} className="translate-y-px" />
          <span className="truncate max-w-[26ch]">{session.directory}</span>
        </span>
        <span className="inline-flex items-baseline gap-1">
          <Clock3 size={10} className="translate-y-px" />
          <span>{formatTime(session.savedAt)}</span>
        </span>
      </div>

      <div className="mt-2 grid gap-2 md:grid-cols-2 text-[11px]">
        <div className="flex items-baseline gap-2 border-l-2 border-border bg-background/40 px-2 py-1">
          <span className="w-[5ch] uppercase tracking-[0.14em] text-muted-foreground-faint">
            turn
          </span>
          <span className={cn('text-foreground/90', isMentor ? 'role-mentor' : 'role-executor')}>
            {session.turn}
          </span>
        </div>
        <div className="flex items-baseline gap-2 border-l-2 border-border bg-background/40 px-2 py-1">
          <span className="w-[5ch] uppercase tracking-[0.14em] text-muted-foreground-faint">
            model
          </span>
          <span className="truncate text-foreground/85">
            <span className="role-mentor">{session.mentorModel.split('/').pop()}</span>
            <span className="mx-0.5 text-muted-foreground-faint">/</span>
            <span className="role-executor">{session.executorModel.split('/').pop()}</span>
          </span>
        </div>
      </div>

      <div className="mt-2 text-[11px] leading-relaxed text-muted-foreground whitespace-pre-wrap break-words [overflow-wrap:anywhere]">
        {session.currentTurnCard?.content || session.spec}
      </div>

      <div className="mt-3 flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex flex-wrap items-baseline gap-1.5 text-[10px] text-muted-foreground">
          <span className="border border-border px-1 py-px">runs {session.runCount}</span>
          {session.hasMentorSession && (
            <span className="border border-role-mentor bg-role-mentor role-mentor px-1 py-px">
              {t('modals.mentorSaved')}
            </span>
          )}
          {session.hasExecutorSession && (
            <span className="border border-role-executor bg-role-executor role-executor px-1 py-px">
              {t('modals.executorSaved')}
            </span>
          )}
        </div>
        <div className="flex flex-col gap-1.5 sm:flex-row">
          <GlassButton
            variant="destructive"
            size="sm"
            onClick={() => onRequestDelete(session)}
            disabled={isRestoring || isDeletionLocked}
          >
            <Trash2 size={11} />
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
            icon={<RotateCcw size={11} />}
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
        <div className="space-y-3 font-mono text-[12px]">
          <p className="text-[11px] leading-relaxed text-muted-foreground">
            · the app found session records from a previous run. restore history to inspect the
            saved messages and state, or explicitly resume execution if you want the agents to keep
            going.
          </p>

          <div className="max-h-[62vh] space-y-2 overflow-y-auto pr-1 scrollbar-thin">
            {sessions.map((session) => (
              <SessionRow
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

          <div className="flex flex-col-reverse gap-2 pt-2 border-t border-border sm:flex-row sm:justify-between">
            <GlassButton variant="ghost" size="sm" onClick={onDismiss}>
              {t('modals.startFresh')}
            </GlassButton>
            <div className="text-[10px] leading-relaxed text-muted-foreground sm:text-right">
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
