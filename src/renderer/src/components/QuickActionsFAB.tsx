import { useTranslation } from 'react-i18next'
import { Plus } from 'lucide-react'
import { cn } from '../lib/utils'

interface QuickActionsFABProps {
  onAction: (actionId: string) => void
  className?: string
}

export function QuickActionsFAB({ onAction, className }: QuickActionsFABProps): React.ReactNode {
  const { t } = useTranslation()

  return (
    <div className={cn('fixed bottom-5 right-5 z-50', className)}>
      <button
        type="button"
        onClick={() => onAction('create')}
        aria-label={t('quickActions.create')}
        title={t('quickActions.create')}
        className="inline-flex h-10 w-10 items-center justify-center border border-foreground bg-foreground text-background rounded-sm transition-colors hover:bg-foreground/90 cursor-pointer"
      >
        <Plus size={16} />
      </button>
    </div>
  )
}
