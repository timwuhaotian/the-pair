import { cn } from '../lib/utils'
import {
  parseAcceptanceVerdictForDisplay,
  parseAcceptanceRecordForDisplay,
  isAcceptanceRecordContent
} from '../lib/acceptance'
import { MarkdownContent } from './MarkdownContent'

function verdictTone(verdict: string): { tone: string; glyph: string } {
  if (verdict === 'pass') return { tone: 'state-done border-state-done bg-state-done', glyph: '✓' }
  return { tone: 'state-running border-state-running bg-state-running', glyph: '!' }
}

function CheckGlyph({ status }: { status: string }): React.ReactNode {
  if (status === 'passed') return <span className="state-done">✓</span>
  if (status === 'failed') return <span className="state-error">✗</span>
  return <span className="text-muted-foreground-faint">·</span>
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

function Tag({
  children,
  tone = 'muted'
}: {
  children: React.ReactNode
  tone?: 'muted' | 'done' | 'running' | 'error'
}): React.ReactNode {
  const toneClass =
    tone === 'done'
      ? 'border-state-done bg-state-done state-done'
      : tone === 'running'
        ? 'border-state-running bg-state-running state-running'
        : tone === 'error'
          ? 'border-state-error bg-state-error state-error'
          : 'border-border bg-background/40 text-muted-foreground'
  return (
    <span
      className={cn(
        'inline-flex items-baseline gap-1 border px-1.5 py-px font-mono text-[10px] uppercase tracking-[0.14em]',
        toneClass
      )}
    >
      {children}
    </span>
  )
}

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

  const v = verdictTone(verdict.verdict)

  return (
    <div className="space-y-2 font-mono">
      <div className="flex flex-wrap items-baseline gap-1.5">
        <span
          className={cn(
            'inline-flex items-baseline gap-1 border px-1.5 py-px text-[10px] uppercase tracking-[0.14em]',
            v.tone
          )}
        >
          <span aria-hidden>{v.glyph}</span>[{verdict.verdict}]
        </span>
        <Tag>risk·{verdict.risk}</Tag>
        <Tag>next·{verdict.nextStep.action}</Tag>
      </div>

      <p className="text-[12px] leading-relaxed text-foreground/90">{verdict.summary}</p>

      <div className="space-y-0.5">
        <div className="text-[10px] uppercase tracking-[0.16em] text-muted-foreground">
          ── evidence ──
        </div>
        <ul className="space-y-0.5 text-[11px] leading-relaxed text-foreground/80">
          {verdict.evidence.map((item) => (
            <li key={item} className="flex items-baseline gap-1.5">
              <span className="text-muted-foreground-faint select-none">·</span>
              <span>{item}</span>
            </li>
          ))}
        </ul>
      </div>

      {verdict.nextStep.instructions.length > 0 && (
        <div className="space-y-0.5">
          <div className="text-[10px] uppercase tracking-[0.16em] text-muted-foreground">
            ── next step ──
          </div>
          <ol className="space-y-0.5 text-[11px] leading-relaxed text-foreground/80">
            {verdict.nextStep.instructions.map((step, index) => (
              <li key={`${index}-${step}`} className="flex items-baseline gap-1.5">
                <span className="text-muted-foreground-faint tabular-nums">{index + 1}.</span>
                <span>{step}</span>
              </li>
            ))}
          </ol>
        </div>
      )}
    </div>
  )
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
    <div className="space-y-2 font-mono">
      <div className="flex flex-wrap items-baseline gap-1.5">
        <Tag>risk·{record.risk}</Tag>
        <Tag>iter·{record.iteration}</Tag>
        <Tag tone={failedCount === 0 ? 'done' : 'error'}>
          {passedCount}/{record.checks.length} passed
        </Tag>
      </div>

      <p className="text-[12px] leading-relaxed text-foreground/90">{record.summary}</p>

      <div className="space-y-1">
        <div className="text-[10px] uppercase tracking-[0.16em] text-muted-foreground">
          ── checks ──
        </div>
        <div className="space-y-0.5">
          {record.checks.map((check, index) => (
            <div key={`${check.name}-${index}`} className="flex items-baseline gap-2 text-[11px]">
              <span aria-hidden className="select-none w-[1ch] tabular-nums">
                <CheckGlyph status={check.status} />
              </span>
              <div className="flex-1 min-w-0">
                <div className="flex items-baseline gap-2">
                  <span className="text-foreground/85 truncate">{check.name}</span>
                  <span className="text-muted-foreground-faint text-[10px] shrink-0">
                    {formatDuration(check.durationMs)}
                  </span>
                </div>
                {check.summary && (
                  <p className="text-[10px] text-muted-foreground mt-0.5">{check.summary}</p>
                )}
              </div>
            </div>
          ))}
        </div>
      </div>

      {record.verdict && (
        <div className="flex flex-wrap items-baseline gap-1.5 pt-1 border-t border-border/40">
          {(() => {
            const v = verdictTone(record.verdict.verdict)
            return (
              <span
                className={cn(
                  'inline-flex items-baseline gap-1 border px-1.5 py-px text-[10px] uppercase tracking-[0.14em]',
                  v.tone
                )}
              >
                <span aria-hidden>{v.glyph}</span>[{record.verdict.verdict}]
              </span>
            )
          })()}
          <span className="text-[11px] text-foreground/80">{record.verdict.summary}</span>
        </div>
      )}
    </div>
  )
}
