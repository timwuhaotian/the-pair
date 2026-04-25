import { ShieldAlert, ShieldCheck, CheckCircle2, XCircle, MinusCircle } from 'lucide-react'
import { cn } from '../lib/utils'
import {
  parseAcceptanceVerdictForDisplay,
  parseAcceptanceRecordForDisplay,
  isAcceptanceRecordContent
} from '../lib/acceptance'
import { MarkdownContent } from './MarkdownContent'

export function AcceptanceMessageBody({ content }: { content: string }): React.ReactNode {
  if (isAcceptanceRecordContent(content)) {
    return <AcceptanceRecordBody content={content} />
  }

  let verdict
  try {
    verdict = parseAcceptanceVerdictForDisplay(content)
  } catch (err) {
    console.warn('[AcceptanceMessageBody] Failed to parse acceptance verdict:', err)
    return <MarkdownContent content={content} />
  }

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <span
          className={cn(
            'inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-[10px] font-black uppercase tracking-[0.16em]',
            verdict.verdict === 'pass'
              ? 'border-emerald-500/20 bg-emerald-500/10 text-emerald-600 dark:text-emerald-300'
              : 'border-amber-500/20 bg-amber-500/10 text-amber-600 dark:text-amber-300'
          )}
        >
          {verdict.verdict === 'pass' ? <ShieldCheck size={11} /> : <ShieldAlert size={11} />}
          {verdict.verdict}
        </span>
        <span className="rounded-full border border-border/40 bg-background/40 px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.16em] text-muted-foreground/80">
          risk {verdict.risk}
        </span>
        <span className="rounded-full border border-border/40 bg-background/40 px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.16em] text-muted-foreground/80">
          next {verdict.nextStep.action}
        </span>
      </div>

      <p className="text-[13px] leading-relaxed text-foreground/90">{verdict.summary}</p>

      <div className="space-y-1">
        <div className="text-[10px] font-bold uppercase tracking-[0.16em] text-muted-foreground">
          Evidence
        </div>
        <ul className="space-y-1 text-[12px] leading-relaxed text-foreground/85">
          {verdict.evidence.map((item) => (
            <li key={item} className="flex gap-2">
              <span className="text-muted-foreground/50">•</span>
              <span>{item}</span>
            </li>
          ))}
        </ul>
      </div>

      {verdict.nextStep.instructions.length > 0 && (
        <div className="space-y-1">
          <div className="text-[10px] font-bold uppercase tracking-[0.16em] text-muted-foreground">
            Next Step
          </div>
          <ol className="space-y-1 text-[12px] leading-relaxed text-foreground/85">
            {verdict.nextStep.instructions.map((step, index) => (
              <li key={`${index}-${step}`} className="flex gap-2">
                <span className="text-muted-foreground/50">{index + 1}.</span>
                <span>{step}</span>
              </li>
            ))}
          </ol>
        </div>
      )}
    </div>
  )
}

function CheckStatusIcon({ status }: { status: string }): React.ReactNode {
  switch (status) {
    case 'passed':
      return <CheckCircle2 size={11} className="text-emerald-500" />
    case 'failed':
      return <XCircle size={11} className="text-red-500" />
    default:
      return <MinusCircle size={11} className="text-muted-foreground/50" />
  }
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

function AcceptanceRecordBody({ content }: { content: string }): React.ReactNode {
  let record
  try {
    record = parseAcceptanceRecordForDisplay(content)
  } catch (err) {
    console.warn('[AcceptanceRecordBody] Failed to parse acceptance record:', err)
    return <MarkdownContent content={content} />
  }

  const passedCount = record.checks.filter((c) => c.status === 'passed').length
  const failedCount = record.checks.filter((c) => c.status === 'failed').length

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="rounded-full border border-border/40 bg-background/40 px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.16em] text-muted-foreground/80">
          risk {record.risk}
        </span>
        <span className="rounded-full border border-border/40 bg-background/40 px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.16em] text-muted-foreground/80">
          iter {record.iteration}
        </span>
        <span
          className={cn(
            'rounded-full border px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.16em]',
            failedCount === 0
              ? 'border-emerald-500/20 bg-emerald-500/10 text-emerald-600 dark:text-emerald-300'
              : 'border-red-500/20 bg-red-500/10 text-red-600 dark:text-red-300'
          )}
        >
          {passedCount}/{record.checks.length} checks passed
        </span>
      </div>

      <p className="text-[13px] leading-relaxed text-foreground/90">{record.summary}</p>

      <div className="space-y-1.5">
        <div className="text-[10px] font-bold uppercase tracking-[0.16em] text-muted-foreground">
          Checks
        </div>
        <div className="space-y-1">
          {record.checks.map((check, index) => (
            <div key={`${check.name}-${index}`} className="flex items-start gap-2 text-[12px]">
              <CheckStatusIcon status={check.status} />
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="font-medium text-foreground/85 truncate">{check.name}</span>
                  <span className="text-muted-foreground/50 text-[10px] shrink-0">
                    {formatDuration(check.durationMs)}
                  </span>
                </div>
                {check.summary && (
                  <p className="text-[11px] text-muted-foreground/70 mt-0.5">{check.summary}</p>
                )}
              </div>
            </div>
          ))}
        </div>
      </div>

      {record.verdict && (
        <div className="flex flex-wrap items-center gap-2 pt-1 border-t border-border/30">
          <span
            className={cn(
              'inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-[10px] font-black uppercase tracking-[0.16em]',
              record.verdict.verdict === 'pass'
                ? 'border-emerald-500/20 bg-emerald-500/10 text-emerald-600 dark:text-emerald-300'
                : 'border-amber-500/20 bg-amber-500/10 text-amber-600 dark:text-amber-300'
            )}
          >
            {record.verdict.verdict === 'pass' ? (
              <ShieldCheck size={11} />
            ) : (
              <ShieldAlert size={11} />
            )}
            {record.verdict.verdict}
          </span>
          <span className="text-[11px] text-foreground/80">{record.verdict.summary}</span>
        </div>
      )}
    </div>
  )
}
