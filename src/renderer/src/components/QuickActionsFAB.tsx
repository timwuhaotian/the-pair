import { motion } from 'framer-motion'
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
    <div className={cn('fixed bottom-6 right-6 z-50', className)}>
      <motion.button
        type="button"
        onClick={() => onAction('create')}
        whileTap={{ scale: 0.94 }}
        whileHover={{ scale: 1.04 }}
        aria-label={t('quickActions.create')}
        title={t('quickActions.create')}
        className={cn(
          'flex h-14 w-14 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-xl transition-colors hover:bg-primary/90'
        )}
      >
        <Plus size={22} />
      </motion.button>
    </div>
  )
}
