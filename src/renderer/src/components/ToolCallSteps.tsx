import React from 'react'
import { motion } from 'framer-motion'
import {
  Terminal,
  FileText,
  Pencil,
  Search,
  CheckCircle2,
  Loader2,
  Circle,
  AlertCircle,
  ChevronDown,
  ChevronRight
} from 'lucide-react'
import { cn } from '../lib/utils'
import type { CognitiveEvent } from '../store/usePairStore'

interface ToolCallStepsProps {
  events: CognitiveEvent[]
  className?: string
}

const TOOL_ICONS: Record<string, React.ElementType> = {
  bash: Terminal,
  shell: Terminal,
  exec: Terminal,
  read: FileText,
  readfile: FileText,
  write: Pencil,
  writefile: Pencil,
  edit: Pencil,
  create: Pencil,
  search: Search,
  grep: Search,
  glob: Search
}

function getToolIcon(toolName: string): React.ElementType {
  const lower = toolName.toLowerCase()
  for (const [key, Icon] of Object.entries(TOOL_ICONS)) {
    if (lower.includes(key)) return Icon
  }
  return Terminal
}

function getStatusIcon(status: CognitiveEvent['status'], isLast: boolean): React.ReactNode {
  if (status === 'completed') return <CheckCircle2 size={12} className="text-green-500" />
  if (status === 'error') return <AlertCircle size={12} className="text-red-500" />
  if (isLast) return <Loader2 size={12} className="text-amber-500 animate-spin" />
  return <Circle size={12} className="text-slate-500/40" />
}

export function ToolCallSteps({ events, className }: ToolCallStepsProps): React.ReactNode {
  const [expanded, setExpanded] = React.useState(false)
  const toolCallEvents = events.filter((e) => e.eventType === 'tool_call')
  const reasoningEvents = events.filter((e) => e.eventType === 'reasoning')
  const displayEvents = [...toolCallEvents, ...reasoningEvents].sort(
    (a, b) => a.timestamp - b.timestamp
  )

  if (displayEvents.length === 0) return null

  const isCollapsed = !expanded
  const visibleEvents = isCollapsed ? displayEvents.slice(-4) : displayEvents

  return (
    <div className={cn('mt-3 space-y-1.5', className)}>
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1 text-[10px] font-medium text-muted-foreground/60 hover:text-muted-foreground transition-colors"
      >
        {expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
        <span>
          {expanded ? '收起' : '展开'}工具调用 ({displayEvents.length})
        </span>
      </button>

      <motion.div initial={false} animate={{ height: 'auto', opacity: 1 }} className="space-y-1">
        {visibleEvents.map((event, index) => {
          const isLast = index === visibleEvents.length - 1
          const toolName = event.toolName ?? ''
          const Icon = event.eventType === 'reasoning' ? Search : getToolIcon(toolName)
          const shortDesc =
            event.description.length > 60
              ? event.description.slice(0, 57) + '...'
              : event.description

          return (
            <div
              key={event.id}
              className={cn(
                'flex items-center gap-2 rounded-lg px-2 py-1 text-xs',
                isLast && !event.status
                  ? 'bg-amber-500/5 border border-amber-500/10'
                  : 'bg-muted/20'
              )}
            >
              <div className="flex-shrink-0">{getStatusIcon(event.status, isLast)}</div>
              <Icon size={12} className="text-muted-foreground/50 flex-shrink-0" />
              <span className="text-muted-foreground/70 truncate flex-1">{shortDesc}</span>
            </div>
          )
        })}
      </motion.div>
    </div>
  )
}
