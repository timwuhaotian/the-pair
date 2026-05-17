import React from 'react'
import { Plus } from 'lucide-react'
import { useTranslation, Trans } from 'react-i18next'
import { GlassButton } from './ui/GlassButton'

interface DashboardEmptyStateProps {
  onCreatePair: () => void
}

export function DashboardEmptyState({ onCreatePair }: DashboardEmptyStateProps): React.ReactNode {
  const { t } = useTranslation()

  return (
    <div className="flex h-full flex-col items-start justify-center px-8 py-8 font-mono">
      <pre className="mb-6 text-[10px] leading-tight text-muted-foreground-faint select-none">
        {`>_ the-pair
   ─────────────
   mentor   · plans, reviews
   handoff  · cross-check
   executor · writes, runs`}
      </pre>

      <h2 className="mb-2 text-[14px] uppercase tracking-[0.18em] text-foreground">
        <span className="text-primary">──</span> {t('emptyState.title')}{' '}
        <span className="text-primary">──</span>
      </h2>
      <p className="mb-6 max-w-[60ch] text-[12px] leading-relaxed text-muted-foreground">
        <Trans
          i18nKey="emptyState.description"
          components={{
            mentor: <span className="role-mentor" />,
            executor: <span className="role-executor" />
          }}
        />
      </p>

      <div className="mb-8 grid w-full max-w-[60ch] grid-cols-3 gap-3 text-[11px]">
        <div className="border border-border bg-background/40 p-3">
          <div className="flex items-baseline gap-1.5 mb-1">
            <span aria-hidden className="role-mentor select-none">
              ●
            </span>
            <span className="role-mentor uppercase tracking-[0.14em] font-bold">
              {t('common.mentor')}
            </span>
          </div>
          <p className="text-[10px] leading-relaxed text-muted-foreground">
            {t('emptyState.mentorDesc')}
          </p>
        </div>
        <div className="border border-border bg-background/40 p-3">
          <div className="flex items-baseline gap-1.5 mb-1">
            <span aria-hidden className="text-muted-foreground select-none">
              ⇄
            </span>
            <span className="text-foreground/85 uppercase tracking-[0.14em] font-bold">
              {t('common.handoff')}
            </span>
          </div>
          <p className="text-[10px] leading-relaxed text-muted-foreground">
            {t('emptyState.handoffDesc')}
          </p>
        </div>
        <div className="border border-border bg-background/40 p-3">
          <div className="flex items-baseline gap-1.5 mb-1">
            <span aria-hidden className="role-executor select-none">
              ●
            </span>
            <span className="role-executor uppercase tracking-[0.14em] font-bold">
              {t('common.executor')}
            </span>
          </div>
          <p className="text-[10px] leading-relaxed text-muted-foreground">
            {t('emptyState.executorDesc')}
          </p>
        </div>
      </div>

      <GlassButton variant="primary" size="md" onClick={onCreatePair} icon={<Plus size={12} />}>
        ▸ {t('emptyState.createFirst').toLowerCase()}
      </GlassButton>
    </div>
  )
}
