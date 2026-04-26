import React from 'react'
import { GlassModal } from './ui/GlassModal'
import { FileText } from 'lucide-react'
import { cn } from '../lib/utils'

interface FileDiffModalProps {
  isOpen: boolean
  onClose: () => void
  filePath: string
  status: string
  diff: string | null
  loading: boolean
  error: string | null
}

function colorizeLine(line: string): { bg: string; fg: string } {
  if (line.startsWith('+') && !line.startsWith('+++'))
    return { bg: 'bg-green-500/10 dark:bg-green-500/5', fg: 'text-green-600 dark:text-green-400' }
  if (line.startsWith('-') && !line.startsWith('---'))
    return { bg: 'bg-red-500/10 dark:bg-red-500/5', fg: 'text-red-600 dark:text-red-400' }
  if (line.startsWith('@@'))
    return { bg: 'bg-blue-500/10 dark:bg-blue-500/5', fg: 'text-blue-600 dark:text-blue-400' }
  if (
    line.startsWith('diff') ||
    line.startsWith('index') ||
    line.startsWith('---') ||
    line.startsWith('+++')
  )
    return { bg: '', fg: 'text-muted-foreground/60' }
  return { bg: '', fg: 'text-muted-foreground/80' }
}

function DiffContent({ diff }: { diff: string }): React.ReactNode {
  const lines = diff.split('\n')
  return (
    <div className="overflow-x-auto">
      {lines.map((line, i) => {
        const { bg, fg } = colorizeLine(line)
        return (
          <div
            key={i}
            className={cn(
              'whitespace-pre font-mono text-[12px] leading-relaxed px-2 py-px',
              bg,
              fg
            )}
          >
            {line || ' '}
          </div>
        )
      })}
    </div>
  )
}

export function FileDiffModal({
  isOpen,
  onClose,
  filePath,
  status,
  diff,
  loading,
  error
}: FileDiffModalProps): React.ReactNode {
  const statusLabel =
    status === '??'
      ? 'Untracked'
      : status === 'A'
        ? 'Added'
        : status === 'D'
          ? 'Deleted'
          : status === 'R'
            ? 'Renamed'
            : 'Modified'

  const statusColor =
    status === 'A' || status === '??'
      ? 'bg-green-500/10 text-green-600 dark:text-green-400'
      : status === 'D'
        ? 'bg-red-500/10 text-red-600 dark:text-red-400'
        : 'bg-yellow-500/10 text-yellow-600 dark:text-yellow-400'

  return (
    <GlassModal isOpen={isOpen} onClose={onClose} title="File Diff" className="max-w-3xl">
      <div className="flex items-center gap-2 mb-3 text-xs text-muted-foreground">
        <FileText size={13} />
        <span className="font-mono truncate">{filePath}</span>
        <span
          className={cn('ml-auto rounded-full px-2 py-0.5 text-[10px] font-medium', statusColor)}
        >
          {statusLabel}
        </span>
      </div>

      <div className="rounded-lg border border-border/30 bg-muted/20 max-h-[60vh] overflow-auto">
        {loading && (
          <div className="flex items-center justify-center py-8 text-muted-foreground">
            <div className="h-4 w-4 rounded-full border-2 border-current border-t-transparent animate-spin mr-2" />
            Loading diff...
          </div>
        )}
        {error && <div className="p-4 text-sm text-red-500">{error}</div>}
        {diff && !loading && <DiffContent diff={diff} />}
      </div>
    </GlassModal>
  )
}
