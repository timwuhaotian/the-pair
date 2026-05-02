import { useTranslation } from 'react-i18next'
import { Plus, FolderOpen, Brain, Zap, RefreshCw } from 'lucide-react'
import { GlassButton } from './ui/GlassButton'
import { GlassCard } from './ui/GlassCard'

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
    { icon: <FolderOpen size={16} />, text: t('emptyState.step1') },
    { icon: <Brain size={16} />, text: t('emptyState.step2') },
    { icon: <Zap size={16} />, text: t('emptyState.step3') },
    { icon: <RefreshCw size={16} />, text: t('emptyState.step4') }
  ]

  return (
    <div className="flex h-full flex-col items-center justify-center p-8">
      <div className="relative mb-6 flex flex-col items-center">
        <div className="glass-card relative flex h-16 w-16 items-center justify-center rounded-2xl border border-border/50 bg-background/80 shadow-xl">
          <Brain size={28} className="text-blue-500" />
          <Zap size={24} className="absolute -right-1 -bottom-1 text-purple-500" />
        </div>
      </div>

      <h2 className="mb-2 text-xl font-semibold text-foreground">{t('emptyState.welcome')}</h2>
      <p className="mb-6 max-w-md text-center text-sm leading-relaxed text-muted-foreground">
        {t('emptyState.description')}
      </p>

      <div className="mb-8 w-full max-w-sm space-y-2">
        {steps.map((step, index) => (
          <GlassCard key={index} className="flex items-center gap-3 p-3">
            <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
              {step.icon}
            </div>
            <span className="text-sm text-foreground">{step.text}</span>
          </GlassCard>
        ))}
      </div>

      <div className="flex flex-col items-center gap-3">
        <GlassButton variant="primary" size="lg" onClick={onCreatePair} icon={<Plus size={16} />}>
          {t('emptyState.createFirst')}
        </GlassButton>
        {onImportConfig && (
          <button
            onClick={onImportConfig}
            className="text-sm text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
          >
            {t('emptyState.importConfig')}
          </button>
        )}
      </div>
    </div>
  )
}
