import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, ChevronRight, RotateCcw, Trash2, Loader2 } from 'lucide-react'
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
    if (lower.includes('permission') || lower.includes('denied'))
      return t('errors.permissionDenied')
    if (lower.includes('timeout')) return t('errors.timeout')
    if (lower.includes('connection') || lower.includes('network')) return t('errors.networkError')
    if (lower.includes('not found') || lower.includes('enoent')) return t('errors.notFound')
    if (lower.includes('locked')) return t('errors.locked')
    if (lower.includes('memory') || lower.includes('oom')) return t('errors.outOfMemory')
    return t('errors.agentError')
  }

  const summary = getErrorSummary(error)
  const isLongError = error.length > 200

  return (
    <div className="border-l-2 border-state-error bg-state-error/8 px-3 py-2 font-mono text-[12px]">
      <div className="mb-1.5 flex items-baseline gap-2">
        <span aria-hidden className="state-error select-none">
          ✗
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[10px] uppercase tracking-[0.16em] state-error">
            {t('errors.executionError')}
          </div>
          <div className="mt-0.5 text-foreground/90">{summary}</div>
        </div>
      </div>

      {isLongError && (
        <>
          <button
            onClick={() => setIsExpanded(!isExpanded)}
            className="flex w-full items-baseline justify-between gap-1.5 mb-1 px-2 py-1 rounded-sm text-[10px] uppercase tracking-[0.14em] text-muted-foreground hover:bg-foreground/[0.05] transition-colors cursor-pointer"
          >
            <span className="flex items-baseline gap-1">
              {isExpanded ? (
                <ChevronDown size={10} className="translate-y-px" />
              ) : (
                <ChevronRight size={10} className="translate-y-px" />
              )}
              <span>{t('errors.viewDetails')}</span>
            </span>
            <span>{isExpanded ? '▴' : '▾'}</span>
          </button>

          {isExpanded && (
            <pre className="mb-2 max-h-[120px] overflow-auto scrollbar-thin border-l-2 border-state-error/30 bg-background/40 px-2 py-1.5 text-[10px] leading-relaxed text-muted-foreground [overflow-wrap:anywhere]">
              {error}
            </pre>
          )}
        </>
      )}

      {!isLongError && (
        <pre className="mb-2 text-[10px] leading-relaxed text-muted-foreground [overflow-wrap:anywhere] whitespace-pre-wrap">
          {error}
        </pre>
      )}

      <div className="flex flex-wrap gap-1.5">
        {onRetry && (
          <GlassButton
            variant="primary"
            size="sm"
            onClick={onRetry}
            disabled={isRetrying}
            data-testid="error-retry-btn"
            icon={
              isRetrying ? <Loader2 size={11} className="animate-spin" /> : <RotateCcw size={11} />
            }
          >
            {isRetrying ? t('errors.retrying') : t('errors.retry')}
          </GlassButton>
        )}
        {onDiscard && (
          <GlassButton variant="ghost" size="sm" onClick={onDiscard} icon={<Trash2 size={11} />}>
            {t('errors.discard')}
          </GlassButton>
        )}
      </div>

      <div className="mt-2 text-[10px] leading-relaxed text-muted-foreground-faint">
        · {t('errors.errorTip')}
      </div>
    </div>
  )
}
