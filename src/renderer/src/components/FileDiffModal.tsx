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

function lineTone(line: string): string {
  if (line.startsWith('+') && !line.startsWith('+++')) return 'state-done bg-state-done'
  if (line.startsWith('-') && !line.startsWith('---')) return 'state-error bg-state-error'
  if (line.startsWith('@@')) return 'role-mentor bg-role-mentor'
  if (
    line.startsWith('diff') ||
    line.startsWith('index') ||
    line.startsWith('---') ||
    line.startsWith('+++')
  )
    return 'text-muted-foreground-faint'
  return 'text-foreground/80'
}

function DiffContent({ diff }: { diff: string }): React.ReactNode {
  const lines = diff.split('\n')
  return (
    <div className="overflow-x-auto">
      {lines.map((line, i) => (
        <div
          key={i}
          className={cn(
            'whitespace-pre font-mono text-[12px] leading-snug px-2 py-px',
            lineTone(line)
          )}
        >
          {line || ' '}
        </div>
      ))}
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
      ? 'untracked'
      : status === 'A'
        ? 'added'
        : status === 'D'
          ? 'deleted'
          : status === 'R'
            ? 'renamed'
            : 'modified'

  const statusTone =
    status === 'A' || status === '??'
      ? 'state-done border-state-done bg-state-done'
      : status === 'D'
        ? 'state-error border-state-error bg-state-error'
        : 'state-running border-state-running bg-state-running'

  return (
    <GlassModal isOpen={isOpen} onClose={onClose} title="file diff" className="max-w-3xl">
      <div className="flex items-baseline gap-2 mb-2 font-mono text-[11px] text-muted-foreground">
        <FileText size={11} className="translate-y-px" />
        <span className="truncate text-foreground/90">{filePath}</span>
        <span
          className={cn(
            'ml-auto border px-1.5 py-px text-[10px] uppercase tracking-[0.14em]',
            statusTone
          )}
        >
          [{statusLabel}]
        </span>
      </div>

      <div className="border border-border bg-background/40 max-h-[60vh] overflow-auto scrollbar-thin rounded-sm">
        {loading && (
          <div className="flex items-baseline justify-start gap-2 px-3 py-4 font-mono text-[11px] text-muted-foreground">
            <span className="state-running tty-blink">●</span>
            <span>· loading diff…</span>
          </div>
        )}
        {error && <div className="px-3 py-2 font-mono text-[11px] state-error">✗ {error}</div>}
        {diff && !loading && <DiffContent diff={diff} />}
      </div>
    </GlassModal>
  )
}
