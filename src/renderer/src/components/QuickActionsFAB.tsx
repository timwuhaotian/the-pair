import { useState, useRef, useEffect } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { Plus, Settings, Book, Keyboard } from 'lucide-react'
import { cn } from '../lib/utils'

interface QuickActionItem {
  id: string
  label: string
  icon: React.ReactNode
  shortcut?: string
}

interface QuickActionsFABProps {
  onAction: (actionId: string) => void
  className?: string
}

const menuVariants = {
  hidden: { opacity: 0, y: 8, scale: 0.95 },
  visible: { opacity: 1, y: 0, scale: 1, transition: { duration: 0.15 } },
  exit: { opacity: 0, y: 8, scale: 0.95, transition: { duration: 0.1 } }
}

export function QuickActionsFAB({ onAction, className }: QuickActionsFABProps): React.ReactNode {
  const { t } = useTranslation()
  const [isOpen, setIsOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setIsOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [])

  const actions: QuickActionItem[] = [
    { id: 'create', label: t('quickActions.create'), icon: <Plus size={16} />, shortcut: '⌘N' },
    { id: 'settings', label: t('quickActions.settings'), icon: <Settings size={16} /> },
    { id: 'help', label: t('quickActions.help'), icon: <Book size={16} /> },
    {
      id: 'shortcuts',
      label: t('quickActions.shortcuts'),
      icon: <Keyboard size={16} />,
      shortcut: '⌘K'
    }
  ]

  return (
    <div ref={menuRef} className={cn('fixed bottom-6 right-6 z-50', className)}>
      <AnimatePresence>
        {isOpen && (
          <motion.div
            variants={menuVariants}
            initial="hidden"
            animate="visible"
            exit="exit"
            className="absolute bottom-14 right-0 w-56 rounded-xl border border-border/50 bg-background/95 p-1 shadow-xl backdrop-blur-xl"
          >
            {actions.map((action) => (
              <button
                key={action.id}
                onClick={() => {
                  onAction(action.id)
                  setIsOpen(false)
                }}
                className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-sm text-foreground hover:bg-muted/50"
              >
                <span className="text-muted-foreground">{action.icon}</span>
                <span className="flex-1">{action.label}</span>
                {action.shortcut && (
                  <span className="text-[10px] font-mono text-muted-foreground">
                    {action.shortcut}
                  </span>
                )}
              </button>
            ))}
          </motion.div>
        )}
      </AnimatePresence>

      <button
        onClick={() => setIsOpen(!isOpen)}
        className={cn(
          'flex h-12 w-12 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-lg hover:bg-primary/90 transition-colors',
          isOpen && 'rotate-45'
        )}
      >
        <Plus size={20} />
      </button>
    </div>
  )
}
