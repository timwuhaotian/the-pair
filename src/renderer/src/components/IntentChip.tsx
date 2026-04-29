import React, { useMemo } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { Search, Pencil, FlaskConical, Eye, Hourglass, AlertTriangle } from 'lucide-react'
import { cn } from '../lib/utils'
import type { CognitiveEvent } from '../store/usePairStore'

interface IntentChipProps {
  events: CognitiveEvent[]
  role: 'mentor' | 'executor'
  className?: string
}

const INTENT_MAP: Record<string, { label: string; Icon: React.ElementType; color: string }> = {
  bash: {
    label: '运行命令',
    Icon: FlaskConical,
    color: 'text-amber-500 bg-amber-500/10 border-amber-500/20'
  },
  read: {
    label: '读取文件',
    Icon: Search,
    color: 'text-blue-500 bg-blue-500/10 border-blue-500/20'
  },
  write: {
    label: '编写代码',
    Icon: Pencil,
    color: 'text-green-500 bg-green-500/10 border-green-500/20'
  },
  edit: {
    label: '编辑文件',
    Icon: Pencil,
    color: 'text-green-500 bg-green-500/10 border-green-500/20'
  },
  search: {
    label: '搜索代码',
    Icon: Search,
    color: 'text-cyan-500 bg-cyan-500/10 border-cyan-500/20'
  },
  grep: {
    label: '搜索代码',
    Icon: Search,
    color: 'text-cyan-500 bg-cyan-500/10 border-cyan-500/20'
  },
  reasoning: {
    label: '推理中',
    Icon: Eye,
    color: 'text-purple-500 bg-purple-500/10 border-purple-500/20'
  },
  error: {
    label: '处理错误',
    Icon: AlertTriangle,
    color: 'text-red-500 bg-red-500/10 border-red-500/20'
  }
}

const FALLBACK_INTENT = {
  label: '处理中',
  Icon: Hourglass,
  color: 'text-slate-500 bg-slate-500/10 border-slate-500/20'
}

function getIntentFromEvents(events: CognitiveEvent[]): {
  label: string
  Icon: React.ElementType
  color: string
} {
  const latest = events[events.length - 1]
  if (!latest) return FALLBACK_INTENT

  if (latest.eventType === 'error') return INTENT_MAP.error
  if (latest.eventType === 'reasoning') return INTENT_MAP.reasoning

  const toolName = latest.toolName?.toLowerCase() ?? ''
  for (const [key, intent] of Object.entries(INTENT_MAP)) {
    if (toolName.includes(key)) return intent
  }

  return FALLBACK_INTENT
}

export function IntentChip({ events, role, className }: IntentChipProps): React.ReactNode {
  const intent = useMemo(() => getIntentFromEvents(events), [events])
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
