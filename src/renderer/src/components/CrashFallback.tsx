import React from 'react'
import { useTranslation } from 'react-i18next'
import { GlassButton } from './ui/GlassButton'

/** Full-screen crash card. Own module so ErrorBoundary.tsx only exports components. */
export function CrashFallback({ error }: { error: Error | null }): React.ReactNode {
  const { t } = useTranslation()

  return (
    <div className="flex h-full w-full items-center justify-center bg-background p-8">
      <div
        role="alert"
        className="shell-card max-w-2xl border-state-error/50 bg-state-error/10 p-5"
      >
        <h2 className="mb-3 text-[14px] font-semibold tracking-tight state-error">
          {t('errors.crashTitle')}
        </h2>
        <pre className="overflow-auto scrollbar-thin rounded-md border border-border bg-background/40 p-3 text-[11px] leading-relaxed text-foreground/85 [overflow-wrap:anywhere]">
          {error?.toString()}
          {'\n\n'}
          {error?.stack}
        </pre>
        <div className="mt-3 flex justify-end">
          <GlassButton variant="primary" size="sm" onClick={() => window.location.reload()}>
            {t('errors.reloadApp')}
          </GlassButton>
        </div>
      </div>
    </div>
  )
}
