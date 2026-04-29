import React, { useMemo } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { Search, Pencil, FlaskConical, Eye, Hourglass, AlertTriangle } from 'lucide-react'
import { cn } from '../lib/utils'
import type { CognitiveEvent } from '../store/usePairStore'

interface IntentChipProps {
  events: CognitiveEvent[]
  role: 'mentor' | 'executor'
  className?: string
}

function getIntentConfig(t: (key: string) => string, events: CognitiveEvent[]) {
  const latest = events[events.length - 1]
  if (!latest)
    return {
      label: t('console.processing'),
      Icon: Hourglass,
      color: 'text-slate-500 bg-slate-500/10 border-slate-500/20'
    }

  if (latest.eventType === 'error')
    return {
      label: t('console.processingError'),
      Icon: AlertTriangle,
      color: 'text-red-500 bg-red-500/10 border-red-500/20'
    }
  if (latest.eventType === 'reasoning')
    return {
      label: t('console.reasoning'),
      Icon: Eye,
      color: 'text-purple-500 bg-purple-500/10 border-purple-500/20'
    }

  const toolName = latest.toolName?.toLowerCase() ?? ''
  if (toolName.includes('bash'))
    return {
      label: t('console.runningCommand'),
      Icon: FlaskConical,
      color: 'text-amber-500 bg-amber-500/10 border-amber-500/20'
    }
  if (toolName.includes('read'))
    return {
      label: t('console.readingFile'),
      Icon: Search,
      color: 'text-blue-500 bg-blue-500/10 border-blue-500/20'
    }
  if (toolName.includes('write'))
    return {
      label: t('console.writingCode'),
      Icon: Pencil,
      color: 'text-green-500 bg-green-500/10 border-green-500/20'
    }
  if (toolName.includes('edit'))
    return {
      label: t('console.editingFile'),
      Icon: Pencil,
      color: 'text-green-500 bg-green-500/10 border-green-500/20'
    }
  if (toolName.includes('search') || toolName.includes('grep'))
    return {
      label: t('console.searchingCode'),
      Icon: Search,
      color: 'text-cyan-500 bg-cyan-500/10 border-cyan-500/20'
    }

  return {
    label: t('console.processing'),
    Icon: Hourglass,
    color: 'text-slate-500 bg-slate-500/10 border-slate-500/20'
  }
}

export function IntentChip({ events, role, className }: IntentChipProps): React.ReactNode {
  const { t } = useTranslation()
  const intent = useMemo(() => getIntentConfig(t, events), [events, t])
  const { label, Icon, color } = intent

  return (
    <AnimatePresence mode="wait">
      <motion.div
        key={`${role}-${events.length}`}
        initial={{ opacity: 0, scale: 0.9, y: 4 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.9, y: -4 }}
        transition={{ duration: 0.2 }}
        className={cn(
          'inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[10px] font-medium',
          color,
          className
        )}
      >
        <Icon size={10} className="animate-pulse" />
        <span>{label}</span>
      </motion.div>
    </AnimatePresence>
  )
}
