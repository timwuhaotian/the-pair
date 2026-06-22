import { save } from '@tauri-apps/plugin-dialog'
import { writeTextFile } from '@tauri-apps/plugin-fs'
import { Marked, type Tokens } from 'marked'
import type { AcceptanceVerdict } from '../types'
import type { TimelineData, TimelineEvent } from './timeline'
import {
  formatDuration,
  formatTokenCount,
  formatTimestamp,
  formatDateTime,
  eventTitle
} from './timeline'

// ── Markdown Report ────────────────────────────────────

export function generateMarkdownReport(timeline: TimelineData): string {
  const lines: string[] = []

  // Header
  lines.push('# Pair Session Report')
  lines.push('')
  lines.push(`**Pair:** ${timeline.pairName}`)
  lines.push(`**Spec:** ${timeline.spec}`)
  lines.push(
    `**Models:** Mentor (${shortModel(timeline.mentorModel)}) / Executor (${shortModel(timeline.executorModel)})`
  )
  lines.push(
    `**Date:** ${formatDateTime(timeline.startedAt)}${timeline.finishedAt ? ` → ${formatDateTime(timeline.finishedAt)}` : ' → now'}`
  )
  lines.push(`**Duration:** ${formatDuration(timeline.durationMs)}`)
  lines.push(`**Status:** ${timeline.status}`)
  lines.push('')

  // Summary
  lines.push('---')
  lines.push('')
  lines.push('## Summary')
  lines.push('')
  lines.push('| Metric | Value |')
  lines.push('|--------|-------|')
  lines.push(`| Iterations | ${timeline.iterations.length} |`)
  lines.push(`| Total Output Tokens | ${formatTokenCount(timeline.totalOutputTokens)} |`)
  lines.push(`| Total Input Tokens | ${formatTokenCount(timeline.totalInputTokens)} |`)
  lines.push(
    `| Mentor Tokens (out/in) | ${formatTokenCount(timeline.mentorOutputTokens)} / ${formatTokenCount(timeline.mentorInputTokens ?? 0)} |`
  )
  lines.push(
    `| Executor Tokens (out/in) | ${formatTokenCount(timeline.executorOutputTokens)} / ${formatTokenCount(timeline.executorInputTokens ?? 0)} |`
  )
  lines.push(`| Files Modified | ${timeline.modifiedFiles.length} |`)
  lines.push('')

  // Timeline
  if (timeline.iterations.length > 0) {
    lines.push('---')
    lines.push('')
    lines.push('## Timeline')
    lines.push('')

    for (const group of timeline.iterations) {
      lines.push(`### Iteration ${group.iteration} (${formatDuration(group.durationMs)})`)
      lines.push('')

      for (const event of group.events) {
        lines.push(
          `**${eventTitle(event.type)}** — ${formatTimestamp(event.timestamp)}${event.tokenUsage ? ` — ${formatTokenCount(event.tokenUsage.outputTokens + (event.tokenUsage.inputTokens ?? 0))} tok` : ''}`
        )
        lines.push('')

        if (event.type === 'acceptance-gate' && event.acceptanceVerdict) {
          const v = event.acceptanceVerdict
          lines.push(
            `**Verdict:** \`${v.verdict.toUpperCase()}\` · **Risk:** \`${v.risk}\` · **Confidence:** ${(v.confidence * 100).toFixed(0)}%`
          )
          lines.push('')
          lines.push(v.summary)
          if (v.reasoning) {
            lines.push('')
            lines.push(v.reasoning)
          }
          if (v.issues.length > 0) {
            lines.push('')
            lines.push('**Issues:**')
            for (const issue of v.issues) {
              lines.push(`- ${issue}`)
            }
          }
          if (v.evidence.length > 0) {
            lines.push('')
            lines.push('**Evidence:**')
            for (const ev of v.evidence) {
              lines.push(`- ${ev}`)
            }
          }
        } else {
          lines.push(event.content || event.summary)
        }

        lines.push('')
      }
    }
  }

  // Modified Files
  if (timeline.modifiedFiles.length > 0) {
    lines.push('---')
    lines.push('')
    lines.push('## Modified Files')
    lines.push('')
    for (const file of timeline.modifiedFiles) {
      lines.push(`- \`${file.status}\` ${file.displayPath}`)
    }
    lines.push('')
  }

  return lines.join('\n')
}

// ── HTML Report ────────────────────────────────────────

export function generateHtmlReport(timeline: TimelineData): string {
  const eventItems = timeline.iterations
    .map(
      (group) => `
    <div class="iteration-group">
      <div class="iteration-header">
        Iteration ${group.iteration}
        <span class="iteration-meta">${formatDuration(group.durationMs)} · ${formatTokenCount(group.totalTokens + group.totalInputTokens)} tok</span>
      </div>
      ${group.events.map((event) => renderHtmlEvent(event)).join('\n      ')}
    </div>`
    )
    .join('\n')

  const filesSection =
    timeline.modifiedFiles.length > 0
      ? `
    <section class="modified-files">
      <h2>Modified Files</h2>
      <ul>
        ${timeline.modifiedFiles.map((f) => `<li><span class="file-status">${escapeHtml(f.status)}</span> ${escapeHtml(f.displayPath)}</li>`).join('\n        ')}
      </ul>
    </section>`
      : ''

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Pair Session Report — ${escapeHtml(timeline.pairName)}</title>
  <style>
    :root {
      --bg: #ffffff;
      --fg: #1a1a2e;
      --muted: #6b7280;
      --border: #e5e7eb;
      --card-bg: #f9fafb;
      --mentor: #3b82f6;
      --executor: #a855f7;
      --human: #22c55e;
      --system: #94a3b8;
      --pass: #10b981;
      --fail: #f59e0b;
    }
    @media (prefers-color-scheme: dark) {
      :root {
        --bg: #0f0f1a;
        --fg: #e5e7eb;
        --muted: #9ca3af;
        --border: #1f2937;
        --card-bg: #1a1a2e;
      }
    }
    * { margin: 0; padding: 0; box-sizing: border-box; }
    body {
      font-family: -apple-system, BlinkMacSystemFont, 'Inter', 'Segoe UI', sans-serif;
      background: var(--bg);
      color: var(--fg);
      line-height: 1.6;
    }
    .report { max-width: 800px; margin: 0 auto; padding: 2.5rem 2rem; }

    .header {
      border-bottom: 1px solid var(--border);
      padding-bottom: 1.5rem;
      margin-bottom: 2rem;
    }
    .header h1 { font-size: 1.5rem; font-weight: 700; margin-bottom: 0.75rem; }
    .header-meta { display: flex; flex-wrap: wrap; gap: 0.5rem 1.5rem; font-size: 0.8125rem; color: var(--muted); }
    .header-meta strong { color: var(--fg); font-weight: 600; }

    .summary-grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
      gap: 0.75rem;
      margin-bottom: 2rem;
    }
    .stat-card {
      background: var(--card-bg);
      border: 1px solid var(--border);
      border-radius: 12px;
      padding: 1rem;
    }
    .stat-label { font-size: 0.6875rem; font-weight: 600; text-transform: uppercase; letter-spacing: 0.08em; color: var(--muted); margin-bottom: 0.25rem; }
    .stat-value { font-size: 1.25rem; font-weight: 700; }

    h2 { font-size: 1rem; font-weight: 700; margin-bottom: 1rem; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); }

    .timeline { position: relative; padding-left: 28px; margin-bottom: 2rem; }
    .timeline::before {
      content: '';
      position: absolute;
      left: 8px;
      top: 8px;
      bottom: 8px;
      width: 2px;
      background: var(--border);
      border-radius: 1px;
    }

    .iteration-group { margin-bottom: 1.5rem; }
    .iteration-group:last-child { margin-bottom: 0; }
    .iteration-header {
      font-size: 0.6875rem;
      font-weight: 700;
      text-transform: uppercase;
      letter-spacing: 0.1em;
      color: var(--muted);
      margin-bottom: 0.75rem;
      padding-bottom: 0.5rem;
      border-bottom: 1px solid var(--border);
    }
    .iteration-meta { font-weight: 400; opacity: 0.7; }

    .event { position: relative; margin-bottom: 1rem; }
    .event:last-child { margin-bottom: 0; }
    .event::before {
      content: '';
      position: absolute;
      left: -24px;
      top: 6px;
      width: 10px;
      height: 10px;
      border-radius: 50%;
      background: var(--dot-color, var(--system));
      border: 2px solid var(--bg);
      box-shadow: 0 0 0 2px var(--dot-color, var(--system));
    }
    .event-title { font-weight: 600; font-size: 0.8125rem; color: var(--dot-color, var(--fg)); }
    .event-time { font-size: 0.6875rem; color: var(--muted); font-family: 'SF Mono', 'Fira Code', monospace; margin-left: 0.5rem; }
    .event-tokens { font-size: 0.6875rem; color: var(--muted); margin-left: 0.5rem; }
    .event-content { font-size: 0.8125rem; color: var(--fg); opacity: 0.85; margin-top: 0.25rem; line-height: 1.5; }
    .event-summary { font-size: 0.75rem; color: var(--muted); margin-top: 0.25rem; border-left: 2px solid var(--border); padding-left: 0.75rem; }
    .event-summary.expandable { cursor: pointer; user-select: none; }
    .event-summary.expandable::after { content: ' ▸ click to expand'; font-size: 0.625rem; opacity: 0.5; }
    .event-summary.expandable.expanded::after { content: ' ▾ click to collapse'; }
    .event-detail { display: none; margin-top: 0.5rem; padding: 0.75rem 1rem; background: var(--card-bg); border: 1px solid var(--border); border-radius: 8px; font-size: 0.8125rem; line-height: 1.7; max-height: 600px; overflow-y: auto; }
    .event-detail.visible { display: block; }
    .event-detail h1, .event-detail h2, .event-detail h3,
    .event-detail h4, .event-detail h5, .event-detail h6 { font-weight: 600; margin: 0.75rem 0 0.25rem; }
    .event-detail h1 { font-size: 1.125rem; }
    .event-detail h2 { font-size: 1rem; }
    .event-detail h3 { font-size: 0.9375rem; }
    .event-detail p { margin: 0.25rem 0; }
    .event-detail pre { background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 0.75rem; overflow-x: auto; font-family: 'SF Mono', 'Fira Code', monospace; font-size: 0.75rem; margin: 0.5rem 0; }
    .event-detail code { font-family: 'SF Mono', 'Fira Code', monospace; font-size: 0.8em; }
    .event-detail :not(pre) > code { background: var(--card-bg); border: 1px solid var(--border); border-radius: 3px; padding: 1px 4px; }
    .event-detail ul, .event-detail ol { padding-left: 1.5rem; margin: 0.25rem 0; }
    .event-detail li { margin: 0.125rem 0; }
    .event-detail blockquote { border-left: 3px solid var(--border); padding-left: 0.75rem; color: var(--muted); margin: 0.5rem 0; }
    .event-detail table { border-collapse: collapse; width: 100%; margin: 0.5rem 0; }
    .event-detail th, .event-detail td { border: 1px solid var(--border); padding: 0.375rem 0.625rem; font-size: 0.75rem; }
    .event-detail th { background: var(--card-bg); font-weight: 600; }

    .badge {
      display: inline-block;
      padding: 2px 8px;
      border-radius: 9999px;
      font-size: 0.6875rem;
      font-weight: 700;
      text-transform: uppercase;
      letter-spacing: 0.04em;
    }
    .badge-pass { background: rgba(16,185,129,0.1); color: var(--pass); border: 1px solid rgba(16,185,129,0.2); }
    .badge-fail { background: rgba(245,158,11,0.1); color: var(--fail); border: 1px solid rgba(245,158,11,0.2); }

    table { width: 100%; border-collapse: collapse; font-size: 0.8125rem; }
    th { text-align: left; padding: 0.5rem 0.75rem; background: var(--card-bg); border-bottom: 1px solid var(--border); font-weight: 600; font-size: 0.6875rem; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); }
    td { padding: 0.5rem 0.75rem; border-bottom: 1px solid var(--border); }

    .modified-files ul { list-style: none; padding: 0; }
    .modified-files li { font-size: 0.8125rem; padding: 0.25rem 0; font-family: 'SF Mono', 'Fira Code', monospace; }
    .file-status { display: inline-block; width: 1.5rem; font-weight: 700; color: var(--muted); }

    .footer { margin-top: 2rem; padding-top: 1rem; border-top: 1px solid var(--border); font-size: 0.6875rem; color: var(--muted); text-align: center; }

    @media print {
      .event { break-inside: avoid; }
      .iteration-group { break-before: auto; }
      body { background: white; color: black; }
      .event-detail { display: block !important; max-height: none !important; }
      .event-summary::after { display: none; }
    }
  </style>
</head>
<body>
  <div class="report">
    <header class="header">
      <h1>${escapeHtml(timeline.pairName)} — Session Report</h1>
      <div class="header-meta">
        <span><strong>Spec:</strong> ${escapeHtml(timeline.spec)}</span>
        <span><strong>Mentor:</strong> ${escapeHtml(shortModel(timeline.mentorModel))}</span>
        <span><strong>Executor:</strong> ${escapeHtml(shortModel(timeline.executorModel))}</span>
        <span><strong>Duration:</strong> ${formatDuration(timeline.durationMs)}</span>
        <span><strong>Status:</strong> ${escapeHtml(timeline.status)}</span>
      </div>
    </header>

    <section class="summary">
      <div class="summary-grid">
        <div class="stat-card">
          <div class="stat-label">Iterations</div>
          <div class="stat-value">${timeline.iterations.length}</div>
        </div>
        <div class="stat-card">
          <div class="stat-label">Total Output</div>
          <div class="stat-value">${formatTokenCount(timeline.totalOutputTokens)}</div>
        </div>
        <div class="stat-card">
          <div class="stat-label">Total Input</div>
          <div class="stat-value">${formatTokenCount(timeline.totalInputTokens ?? 0)}</div>
        </div>
        <div class="stat-card">
          <div class="stat-label">Mentor (out/in)</div>
          <div class="stat-value">${formatTokenCount(timeline.mentorOutputTokens)} / ${formatTokenCount(timeline.mentorInputTokens ?? 0)}</div>
        </div>
        <div class="stat-card">
          <div class="stat-label">Executor (out/in)</div>
          <div class="stat-value">${formatTokenCount(timeline.executorOutputTokens)} / ${formatTokenCount(timeline.executorInputTokens ?? 0)}</div>
        </div>
        <div class="stat-card">
          <div class="stat-label">Files Modified</div>
          <div class="stat-value">${timeline.modifiedFiles.length}</div>
        </div>
      </div>
    </section>

    <section class="timeline">
      <h2>Timeline</h2>
      <div class="timeline">
      ${eventItems}
      </div>
    </section>

    ${filesSection}

    <div class="footer">
      Generated by The Pair · ${formatDateTime(Date.now())}
    </div>
  </div>
  <script>
document.querySelectorAll('.event-summary').forEach(el => {
  el.addEventListener('click', () => {
    const detail = el.nextElementSibling;
    if (detail && detail.classList.contains('event-detail')) {
      detail.classList.toggle('visible');
      el.classList.toggle('expanded');
    }
  });
});
</script>
</body>
</html>`
}

// ── File Export ────────────────────────────────────────

export async function exportAsHtml(timeline: TimelineData): Promise<void> {
  const html = generateHtmlReport(timeline)
  const defaultName = `pair-report-${timeline.pairName.toLowerCase().replace(/\s+/g, '-')}-${new Date().toISOString().slice(0, 10)}.html`

  const filePath = await save({
    defaultPath: defaultName,
    filters: [{ name: 'HTML', extensions: ['html'] }]
  })

  if (!filePath) return

  await writeTextFile(filePath, html)
}

// ── Clipboard ──────────────────────────────────────────

export async function copyMarkdownReport(timeline: TimelineData): Promise<boolean> {
  const md = generateMarkdownReport(timeline)
  try {
    await navigator.clipboard.writeText(md)
    return true
  } catch {
    return false
  }
}

// ── Internal helpers ───────────────────────────────────

function shortModel(model: string): string {
  return model.split('/').pop() ?? model
}

function dotColor(event: TimelineEvent): string {
  switch (event.type) {
    case 'mentor-plan':
    case 'mentor-review':
      return 'var(--mentor)'
    case 'executor-result':
      return 'var(--executor)'
    case 'human-feedback':
      return 'var(--human)'
    case 'acceptance-gate':
      return event.acceptanceVerdict?.verdict === 'pass' ? 'var(--pass)' : 'var(--fail)'
    case 'handoff':
      return 'var(--system)'
  }
}

/** Protocols that can execute script when a link/image href is opened. */
function isUnsafeHref(href: string): boolean {
  const value = href.trim().toLowerCase()
  return (
    value.startsWith('javascript:') ||
    value.startsWith('vbscript:') ||
    value.startsWith('data:text/html')
  )
}

/**
 * Dedicated marked instance for exported reports. Agent/model output is rendered
 * as markdown into a standalone HTML file the user opens in a browser, so it must
 * not be able to inject executable markup. We escape raw HTML passthrough (e.g.
 * `<script>`, `<img onerror=…>`) and neutralize script-bearing URL protocols in
 * links/images. Markdown-generated HTML (headings, code, tables) is unaffected.
 */
const reportMarkdown = new Marked({
  breaks: true,
  walkTokens: (token) => {
    if (token.type === 'link' || token.type === 'image') {
      const node = token as Tokens.Link | Tokens.Image
      if (typeof node.href === 'string' && isUnsafeHref(node.href)) {
        node.href = '#'
      }
    }
  },
  renderer: {
    html: ({ text }: Tokens.HTML | Tokens.Tag): string => escapeHtml(text)
  }
})

function renderMarkdown(md: string): string {
  if (!md) return ''
  return reportMarkdown.parse(md, { async: false }) as string
}

function renderHtmlEvent(event: TimelineEvent): string {
  const isAcceptanceGate = event.type === 'acceptance-gate' && event.acceptanceVerdict

  const badges = event.acceptanceVerdict
    ? ` <span class="badge badge-${event.acceptanceVerdict.verdict}">${event.acceptanceVerdict.verdict.toUpperCase()}</span> <span class="badge" style="background:var(--card-bg);color:var(--muted);border:1px solid var(--border)">${escapeHtml(event.acceptanceVerdict.risk)}</span>`
    : ''

  let summaryText: string
  let detailHtml: string
  let hasDetail: boolean

  if (isAcceptanceGate) {
    const v = event.acceptanceVerdict!
    summaryText = v.summary
    detailHtml = renderAcceptanceVerdictHtml(v)
    hasDetail = true
  } else {
    const hasFullContent =
      event.content && event.content !== event.summary && event.content.length > 120
    summaryText = event.summary
    detailHtml = hasFullContent ? renderMarkdown(event.content) : ''
    hasDetail = !!hasFullContent
  }

  const detailSection = hasDetail ? `\n        <div class="event-detail">${detailHtml}</div>` : ''

  return `
      <div class="event" style="--dot-color: ${dotColor(event)}">
        <div>
          <span class="event-title">${eventTitle(event.type)}</span>
          <span class="event-time">${formatTimestamp(event.timestamp)}</span>${event.tokenUsage ? `<span class="event-tokens">${formatTokenCount(event.tokenUsage.outputTokens + (event.tokenUsage.inputTokens ?? 0))} tok</span>` : ''}
          ${badges}
        </div>
        <div class="event-summary${hasDetail ? ' expandable' : ''}">${escapeHtml(summaryText)}</div>${detailSection}
      </div>`
}

function renderAcceptanceVerdictHtml(verdict: AcceptanceVerdict): string {
  const parts: string[] = []
  parts.push(
    `<p><strong>Verdict:</strong> ${verdict.verdict.toUpperCase()} &middot; <strong>Risk:</strong> ${verdict.risk} &middot; <strong>Confidence:</strong> ${(verdict.confidence * 100).toFixed(0)}%</p>`
  )
  parts.push(`<p>${escapeHtml(verdict.summary)}</p>`)
  if (verdict.reasoning) {
    parts.push(`<p><strong>Reasoning:</strong> ${escapeHtml(verdict.reasoning)}</p>`)
  }
  if (verdict.issues.length > 0) {
    parts.push('<p><strong>Issues:</strong></p><ul>')
    for (const issue of verdict.issues) {
      parts.push(`<li>${escapeHtml(issue)}</li>`)
    }
    parts.push('</ul>')
  }
  if (verdict.evidence.length > 0) {
    parts.push('<p><strong>Evidence:</strong></p><ul>')
    for (const ev of verdict.evidence) {
      parts.push(`<li>${escapeHtml(ev)}</li>`)
    }
    parts.push('</ul>')
  }
  if (verdict.nextStep.instructions.length > 0) {
    parts.push('<p><strong>Next Steps:</strong></p><ol>')
    for (const inst of verdict.nextStep.instructions) {
      parts.push(`<li>${escapeHtml(inst)}</li>`)
    }
    parts.push('</ol>')
  }
  return parts.join('\n')
}

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}
