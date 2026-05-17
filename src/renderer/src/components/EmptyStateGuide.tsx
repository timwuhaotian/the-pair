import { useTranslation } from 'react-i18next'
import { Plus } from 'lucide-react'
import { GlassButton } from './ui/GlassButton'

interface EmptyStateGuideProps {
  onCreatePair: () => void
  onImportConfig?: () => void
}

export function EmptyStateGuide({
  onCreatePair,
  onImportConfig
}: EmptyStateGuideProps): React.ReactNode {
  const { t } = useTranslation()

  const steps = [
    { n: '01', text: t('emptyState.step1') },
    { n: '02', text: t('emptyState.step2') },
    { n: '03', text: t('emptyState.step3') },
    { n: '04', text: t('emptyState.step4') }
  ]

  return (
    <div className="flex h-full flex-col items-start justify-center px-8 py-8 font-mono">
      <pre className="mb-4 text-[10px] leading-tight text-muted-foreground-faint select-none">
        {`>_ welcome to the-pair
   first run — let's get you set up`}
      </pre>

      <h2 className="mb-1 text-[14px] uppercase tracking-[0.18em] text-foreground">
        <span className="text-primary">──</span> {t('emptyState.welcome')}{' '}
        <span className="text-primary">──</span>
      </h2>
      <p className="mb-5 max-w-[60ch] text-[12px] leading-relaxed text-muted-foreground">
        {t('emptyState.description')}
      </p>

      <div className="mb-6 w-full max-w-[60ch] space-y-1">
        {steps.map((step) => (
          <div
            key={step.n}
            className="flex items-baseline gap-2 border-l-2 border-border bg-background/40 px-3 py-1.5 text-[12px]"
          >
            <span className="text-muted-foreground-faint tabular-nums w-[2ch]">{step.n}</span>
            <span aria-hidden className="text-muted-foreground-faint">
              ▸
            </span>
            <span className="text-foreground/90">{step.text}</span>
          </div>
        ))}
      </div>

      <div className="flex items-baseline gap-3">
        <GlassButton variant="primary" size="md" onClick={onCreatePair} icon={<Plus size={12} />}>
          ▸ {t('emptyState.createFirst').toLowerCase()}
        </GlassButton>
        {onImportConfig && (
          <button
            onClick={onImportConfig}
            className="text-[11px] uppercase tracking-[0.14em] text-muted-foreground hover:text-foreground underline-offset-4 hover:underline transition-colors cursor-pointer"
          >
            · {t('emptyState.importConfig')}
          </button>
        )}
      </div>
    </div>
  )
}
