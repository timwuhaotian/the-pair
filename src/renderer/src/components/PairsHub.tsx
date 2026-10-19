import React from 'react'
import { useTranslation } from 'react-i18next'
import { Folder, Pause, Play, Plus, Trash2 } from 'lucide-react'
import { motion } from 'framer-motion'
import { type Pair } from '../store/usePairStore'
import { buildWorkspaceGroups } from '../lib/dashboardPairs'
import { fadeInUp } from '../lib/animations'
import { isPairActive } from '../lib/pairStatus'
import { cn, formatIterations } from '../lib/utils'
import { StatusBadge } from './StatusBadge'
import { StartupHero } from './StartupHero'
import { RecoveryBanner } from './RecoveryBanner'
import { GlassButton } from './ui/GlassButton'
import { IconButton } from './ui/IconButton'
import { sectionLabel } from '../lib/ui'
import { StatCards } from './StatCards'
import { buildPairInsights } from '../lib/dashboardPairs'

interface PairsHubProps {
  pairs: Pair[]
  onSelectPair: (pair: Pair) => void
  onDeletePair: (pair: Pair) => void
  onPausePair: (pairId: string) => void
  onResumePair: (pairId: string) => void
  onCreatePair: () => void
  deletingPairId: string | null
}

function PairCard({
  pair,
  onSelectPair,
  onDeletePair,
  onPausePair,
  onResumePair,
  deletingPairId,
  className
}: {
  pair: Pair
  onSelectPair: (pair: Pair) => void
  onDeletePair: (pair: Pair) => void
  onPausePair: (pairId: string) => void
  onResumePair: (pairId: string) => void
  deletingPairId: string | null
  className?: string
}): React.ReactNode {
  const { t } = useTranslation()
  const stalled =
    (pair.turn === 'mentor' && pair.mentorActivity.phase === 'stalled') ||
    (pair.turn === 'executor' && pair.executorActivity.phase === 'stalled')

  return (
    <motion.div
      variants={fadeInUp}
      initial="hidden"
      animate="visible"
      data-testid={`hub-pair-card-${pair.id}`}
      className={cn(
        'shell-card shell-card-hover group relative flex w-full flex-col gap-2 px-4 py-3',
        className
      )}
    >
      <div className="flex min-w-0 items-center gap-2">
        {/* The name is the card's primary action; its ::after stretches over the
            whole card so the card stays one click target without nesting the
            pause/delete buttons inside another button. That also makes it the
            hover target for every row, so the card's tooltip lives here. */}
        <button
          type="button"
          onClick={() => onSelectPair(pair)}
          title={pair.spec || pair.directory || undefined}
          className={cn(
            'min-w-0 flex-1 cursor-pointer truncate text-left text-[14px] font-medium text-foreground',
            "after:absolute after:inset-0 after:rounded-[inherit] after:content-['']",
            'focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-offset-2 focus-visible:after:outline-ring'
          )}
        >
          {pair.name}
        </button>
        <span className="relative z-10 flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
          {pair.status === 'Paused' ? (
            <IconButton
              aria-label={t('common.resume', { defaultValue: 'Resume' })}
              onClick={() => onResumePair(pair.id)}
            >
              <Play size={12} />
            </IconButton>
          ) : isPairActive(pair.status) ? (
            <IconButton
              aria-label={t('common.pause', { defaultValue: 'Pause' })}
              onClick={() => onPausePair(pair.id)}
            >
              <Pause size={12} />
            </IconButton>
          ) : null}
          <IconButton
            aria-label={t('common.delete', { defaultValue: 'Delete' })}
            onClick={() => onDeletePair(pair)}
            disabled={deletingPairId === pair.id}
            destructive
            data-testid={`hub-pair-delete-${pair.id}`}
          >
            <Trash2 size={12} />
          </IconButton>
        </span>
      </div>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <StatusBadge status={pair.status} stalled={stalled} />
        <span className="font-mono text-[11px] tabular-nums text-muted-foreground-faint">
          {formatIterations(pair.iterations, pair.maxIterations)}
        </span>
        {(pair.worktreeBranch ?? pair.branch) && (
          <span className="inline-flex min-w-0 items-baseline gap-1 font-mono text-[11px] text-muted-foreground">
            <Folder size={10} className="shrink-0 translate-y-px" />
            <span className="truncate">{pair.worktreeBranch ?? pair.branch}</span>
          </span>
        )}
      </div>
      {(pair.spec || pair.directory) && (
        <p className="truncate text-[12px] text-muted-foreground">{pair.spec || pair.directory}</p>
      )}
    </motion.div>
  )
}

export function PairsHub({
  pairs,
  onSelectPair,
  onDeletePair,
  onPausePair,
  onResumePair,
  onCreatePair,
  deletingPairId
}: PairsHubProps): React.ReactNode {
  const { t } = useTranslation()
  const workspaceGroups = buildWorkspaceGroups(pairs)
  const insights = buildPairInsights(pairs)
  const [recoveryDismissed, setRecoveryDismissed] = React.useState(false)
  // Snapshot rehydration at startup is the crash recovery path — surface the
  // restored pairs that need a human decision as a dismissible banner.
  const recoverablePairs = React.useMemo(
    () =>
      pairs.filter(
        (p) => p.status === 'Paused' || p.status === 'Awaiting Human Review' || p.status === 'Error'
      ),
    [pairs]
  )
  const showRecovery = !recoveryDismissed && recoverablePairs.length > 0

  if (pairs.length === 0) {
    return (
      <div
        data-testid="pairs-hub-empty"
        className="flex h-full items-center justify-center overflow-y-auto px-6 py-8"
      >
        <div className="flex w-full max-w-[760px] flex-col items-center gap-8 md:flex-row md:items-center md:justify-center md:gap-10">
          <div className="shrink-0">
            <StartupHero
              size="md"
              animated
              tagline={t('startup.tagline')}
              wordmark={t('startup.wordmark')}
              caption={t('startup.firstRunCaption')}
            />
          </div>
          <div className="flex w-full max-w-[320px] flex-col gap-3 text-[13px] md:border-l md:border-border md:pl-10">
            <h2 className={sectionLabel}>{t('startup.consoleEmptyTitle')}</h2>
            <div className="shell-card flex flex-col gap-2 p-3 text-[12px] leading-relaxed">
              <div className="flex items-baseline gap-2">
                <span aria-hidden className="role-mentor select-none">
                  ●
                </span>
                <span className="role-mentor font-bold uppercase tracking-[0.14em]">
                  {t('common.mentor')}
                </span>
                <span className="text-muted-foreground">· {t('emptyState.mentorDesc')}</span>
              </div>
              <div className="flex items-baseline gap-2">
                <span aria-hidden className="text-muted-foreground select-none">
                  ⇄
                </span>
                <span className="text-foreground/85 font-bold uppercase tracking-[0.14em]">
                  {t('common.handoff')}
                </span>
                <span className="text-muted-foreground">· {t('emptyState.handoffDesc')}</span>
              </div>
              <div className="flex items-baseline gap-2">
                <span aria-hidden className="role-executor select-none">
                  ●
                </span>
                <span className="role-executor font-bold uppercase tracking-[0.14em]">
                  {t('common.executor')}
                </span>
                <span className="text-muted-foreground">· {t('emptyState.executorDesc')}</span>
              </div>
            </div>
            <p className="text-[13px] leading-relaxed text-muted-foreground">
              {t('startup.firstRunHint')}
            </p>
            <GlassButton
              variant="primary"
              size="lg"
              icon={<Plus size={14} />}
              onClick={onCreatePair}
              data-testid="hub-empty-create"
              className="mt-1 self-start"
            >
              {t('emptyState.createFirst')}
            </GlassButton>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div data-testid="pairs-hub" className="flex h-full flex-col overflow-hidden">
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-3 px-6 pt-5">
        <div>
          <h1 className="text-[17px] font-semibold tracking-tight text-foreground">
            {t('hub.title')}
          </h1>
          <p className="mt-0.5 text-[13px] text-muted-foreground">{t('hub.subtitle')}</p>
        </div>
        <GlassButton
          variant="primary"
          size="sm"
          icon={<Plus size={12} />}
          onClick={onCreatePair}
          data-testid="hub-new-pair"
        >
          {t('quickActions.create')}
        </GlassButton>
      </div>

      <div className="shrink-0 px-6 pt-4">
        <StatCards
          total={pairs.length}
          running={insights.running}
          paused={pairs.filter((p) => p.status === 'Paused').length}
          finished={pairs.filter((p) => p.status === 'Finished').length}
        />
      </div>

      {showRecovery && (
        <div className="shrink-0 px-6 pt-3">
          <RecoveryBanner
            pairs={recoverablePairs}
            onOpenPair={onSelectPair}
            onDismiss={() => setRecoveryDismissed(true)}
          />
        </div>
      )}

      <div className="min-h-0 flex-1 space-y-6 overflow-y-auto px-6 py-5 scrollbar-thin">
        {workspaceGroups.map((workspace) => (
          <section key={workspace.directory} className="space-y-2">
            <header
              className="flex items-baseline justify-between gap-2 border-b border-border/40 px-1 pb-1"
              title={workspace.directory}
            >
              <div className="flex min-w-0 items-baseline gap-1.5">
                <Folder size={11} className="shrink-0 translate-y-px text-muted-foreground" />
                <span className="truncate text-[13px] font-medium text-foreground/90">
                  {workspace.shortName}
                </span>
                <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground-faint">
                  ({workspace.pairs.length})
                </span>
              </div>
              <span className="hidden max-w-[28ch] truncate font-mono text-[11px] text-muted-foreground-faint lg:inline">
                {workspace.directory}
              </span>
            </header>
            {workspace.statusGroups.map((group) => (
              <div key={`${workspace.directory}::${group.key}`} className="space-y-2">
                <h3 className={cn(sectionLabel, 'pl-1')}>
                  {t(group.titleKey)}{' '}
                  <span className="tabular-nums text-muted-foreground-faint">
                    ({group.pairs.length})
                  </span>
                </h3>
                <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
                  {group.pairs.map((pair) => (
                    <PairCard
                      key={pair.id}
                      pair={pair}
                      className={group.pairs.length === 1 ? 'xl:col-span-2' : undefined}
                      onSelectPair={onSelectPair}
                      onDeletePair={onDeletePair}
                      onPausePair={onPausePair}
                      onResumePair={onResumePair}
                      deletingPairId={deletingPairId}
                    />
                  ))}
                </div>
              </div>
            ))}
          </section>
        ))}
      </div>
    </div>
  )
}
