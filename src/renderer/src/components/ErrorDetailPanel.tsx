import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AlertTriangle, ChevronDown, ChevronUp, RotateCcw, Trash2, Loader2 } from 'lucide-react'
import { GlassButton } from './ui/GlassButton'

interface ErrorDetailPanelProps {
  error: string | null
  onRetry?: () => void
  onDiscard?: () => void
  isRetrying?: boolean
}

export function ErrorDetailPanel({
  error,
  onRetry,
  onDiscard,
  isRetrying = false
}: ErrorDetailPanelProps): React.ReactNode {
  const { t } = useTranslation()
  const [isExpanded, setIsExpanded] = useState(false)

  if (!error) return null

  const getErrorSummary = (err: string): string => {
    const lower = err.toLowerCase()
    if (lower.includes('permission') || lower.includes('denied')) {
      return t('errors.permissionDenied')
    }
    if (lower.includes('timeout')) {
      return t('errors.timeout')
    }
    if (lower.includes('connection') || lower.includes('network')) {
      return t('errors.networkError')
    }
    if (lower.includes('not found') || lower.includes('enoent')) {
      return t('errors.notFound')
    }
    if (lower.includes('locked')) {
      return t('errors.locked')
    }
    if (lower.includes('memory') || lower.includes('oom')) {
      return t('errors.outOfMemory')
    }
    return t('errors.agentError')
  }

  const summary = getErrorSummary(error)
  const isLongError = error.length > 200

  return (
    <div className="rounded-2xl border border-red-500/20 bg-red-500/8 p-4">
      <div className="mb-3 flex items-start gap-3">
        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border border-red-500/25 bg-red-500/10">
          <AlertTriangle size={16} className="text-red-600 dark:text-red-400" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[10px] font-semibold uppercase tracking-[0.18em] text-red-600/80 dark:text-red-400/80">
            {t('errors.executionError')}
          </div>
          <div className="mt-1 text-sm font-medium text-foreground">{summary}</div>
        </div>
      </div>

      {isLongError && (
        <>
          <button
            onClick={() => setIsExpanded(!isExpanded)}
            className="mb-3 flex w-full items-center justify-between rounded-xl border border-border/40 bg-background/40 px-3 py-2 text-xs text-muted-foreground transition-colors hover:bg-background/60 cursor-pointer"
          >
            <span>{t('errors.viewDetails')}</span>
            {isExpanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
          </button>

          {isExpanded && (
            <pre className="mb-3 max-h-[120px] overflow-auto rounded-xl border border-border/40 bg-background/60 p-3 font-mono text-[11px] leading-relaxed text-muted-foreground">
              {error}
            </pre>
          )}
        </>
      )}

      {!isLongError && (
        <pre className="mb-3 font-mono text-[11px] leading-relaxed text-muted-foreground/80">
          {error}
        </pre>
      )}

      <div className="flex flex-wrap gap-2">
        {onRetry && (
          <GlassButton
            variant="primary"
            size="sm"
            onClick={onRetry}
            disabled={isRetrying}
            icon={
              isRetrying ? <Loader2 size={12} className="animate-spin" /> : <RotateCcw size={12} />
            }
          >
            {isRetrying ? t('errors.retrying') : t('errors.retry')}
          </GlassButton>
        )}
        {onDiscard && (
          <GlassButton variant="ghost" size="sm" onClick={onDiscard} icon={<Trash2 size={12} />}>
            {t('errors.discard')}
          </GlassButton>
        )}
      </div>

      <div className="mt-3 rounded-xl border border-border/30 bg-muted/20 p-2.5 text-[10px] leading-relaxed text-muted-foreground">
        {t('errors.errorTip')}
      </div>
    </div>
  )
}
