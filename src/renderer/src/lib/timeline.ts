import type { AcceptanceRecord, AcceptanceVerdict, TurnTokenUsage } from '../types'
import { stripSystemPrompt } from './utils'
import { isAcceptanceVerdictContent, parseAcceptanceVerdictForDisplay } from './acceptance'
import { collapseConsecutiveConsoleMessages } from './consoleMessages'

// ── Types ──────────────────────────────────────────────

export type TimelineEventType =
  | 'mentor-plan'
  | 'executor-result'
  | 'mentor-review'
  | 'human-feedback'
  | 'acceptance-gate'
  | 'handoff'

export interface TimelineEvent {
  id: string
  type: TimelineEventType
  iteration: number
  from: 'mentor' | 'executor' | 'human' | 'system'
  timestamp: number
  title: string
  summary: string
  content: string
  tokenUsage?: TurnTokenUsage
  durationMs?: number
  acceptanceVerdict?: AcceptanceVerdict
}

export interface IterationGroup {
  iteration: number
  events: TimelineEvent[]
  startedAt: number
  endedAt: number
  durationMs: number
  totalTokens: number
  totalInputTokens: number
}

export interface TimelineData {
  pairName: string
  spec: string
  mentorModel: string
  executorModel: string
  startedAt: number
  finishedAt?: number
  status: string
  iterations: IterationGroup[]
  totalOutputTokens: number
  totalInputTokens: number
  mentorOutputTokens: number
  mentorInputTokens: number
  executorOutputTokens: number
  executorInputTokens: number
  acceptanceRecords: AcceptanceRecord[]
  modifiedFiles: Array<{ path: string; status: string; displayPath: string }>
  durationMs: number
}

// ── Input types (mirror store types to avoid circular deps) ──

interface TimelineMessage {
  id: string
  timestamp: number
  from: 'mentor' | 'executor' | 'human'
  to: string
  type: 'plan' | 'feedback' | 'progress' | 'result' | 'question' | 'handoff' | 'acceptance'
  content: string
  iteration: number
  tokenUsage?: TurnTokenUsage
}

interface TimelinePair {
  name: string
  spec: string
  mentorModel: string
  executorModel: string
  status: string
  messages: TimelineMessage[]
  latestAcceptance?: AcceptanceRecord
  modifiedFiles: Array<{ path: string; status: string; displayPath: string }>
  currentRunStartedAt: number
  currentRunFinishedAt?: number
}

// ── Helpers ────────────────────────────────────────────

export function isTechnicalHandoff(content: string): boolean {
  return (
    content.includes('### ROLE:') ||
    content.includes('--- COMMAND TO EXECUTE ---') ||
    content.includes('--- REVIEW REQUEST ---')
  )
}

function isEmptyContent(content: string): boolean {
  return !content || content.trim() === '{}' || content.trim() === '[]'
}

function summarizeEvent(content: string): string {
  const cleaned = stripSystemPrompt(content.trim())
  if (!cleaned) return ''
  if (cleaned.length <= 120) return cleaned
  return cleaned.slice(0, 117) + '...'
}

function classifyEventType(msg: TimelineMessage): TimelineEventType {
  if (msg.type === 'acceptance') return 'acceptance-gate'
  if (msg.type === 'feedback') return 'human-feedback'
  if (msg.type === 'handoff') return 'handoff'
  if (msg.from === 'mentor' && msg.type === 'result') return 'mentor-review'
  if (msg.from === 'mentor') return 'mentor-plan'
  if (msg.from === 'executor') return 'executor-result'
  return 'handoff'
}

export function eventTitle(type: TimelineEventType): string {
  switch (type) {
    case 'mentor-plan':
      return 'Mentor Plan'
    case 'executor-result':
      return 'Executor Execution'
    case 'mentor-review':
      return 'Mentor Review'
    case 'human-feedback':
      return 'Human Feedback'
    case 'acceptance-gate':
      return 'Acceptance Gate'
    case 'handoff':
      return 'Handoff'
  }
}

function tryParseAcceptance(content: string): AcceptanceVerdict | null {
  try {
    if (isAcceptanceVerdictContent(content)) {
      return parseAcceptanceVerdictForDisplay(content)
    }
  } catch {
    // not parseable, skip
  }
  return null
}

function sumTokensForRole(
  messages: TimelineMessage[],
  role: 'mentor' | 'executor',
  field: 'outputTokens' | 'inputTokens'
): number {
  return messages.reduce((sum, msg) => {
    if (msg.from === role && msg.tokenUsage?.[field]) {
      return sum + msg.tokenUsage[field]!
    }
    return sum
  }, 0)
}

function groupMessagesByIteration(messages: TimelineMessage[]): Map<number, TimelineMessage[]> {
  const groups = new Map<number, TimelineMessage[]>()
  for (const msg of messages) {
    const existing = groups.get(msg.iteration) ?? []
    existing.push(msg)
    groups.set(msg.iteration, existing)
  }
  return groups
}

// ── Main builder ───────────────────────────────────────

export function buildTimeline(messages: TimelineMessage[], pair: TimelinePair): TimelineData {
  // Filter out noise
  const filtered = collapseConsecutiveConsoleMessages(
    messages.filter((msg) => {
      if (msg.type === 'handoff') return false
      if (!msg.content || isEmptyContent(msg.content)) return false
      if (msg.type === 'plan' || msg.type === 'result' || msg.type === 'acceptance') return true
      if (isTechnicalHandoff(msg.content) && msg.from !== 'human') return false
      return true
    })
  )

  // Group by iteration
  const iterationMap = groupMessagesByIteration(filtered)
  const sortedIterations = [...iterationMap.keys()].sort((a, b) => a - b)

  const iterations: IterationGroup[] = sortedIterations.map((iter) => {
    const msgs = iterationMap.get(iter)!
    const sorted = [...msgs].sort((a, b) => a.timestamp - b.timestamp)

    const startedAt = sorted[0]?.timestamp ?? 0
    const endedAt = sorted[sorted.length - 1]?.timestamp ?? 0

    // Compute per-event durations (time to next event in same iteration)
    const events: TimelineEvent[] = sorted.map((msg, eventIdx) => {
      const nextTs = sorted[eventIdx + 1]?.timestamp
      const type = classifyEventType(msg)
      const displayContent =
        msg.from === 'human' ? stripSystemPrompt(msg.content.trim()) : msg.content.trim()
      const acceptance = type === 'acceptance-gate' ? tryParseAcceptance(displayContent) : undefined

      return {
        id: msg.id,
        type,
        iteration: iter,
        from: msg.from,
        timestamp: msg.timestamp,
        title: eventTitle(type),
        summary: summarizeEvent(displayContent),
        content: displayContent,
        tokenUsage: msg.tokenUsage,
        durationMs: nextTs ? nextTs - msg.timestamp : undefined,
        acceptanceVerdict: acceptance ?? undefined
      }
    })

    const totalTokens = msgs.reduce((sum, m) => sum + (m.tokenUsage?.outputTokens ?? 0), 0)
    const totalInputTokens = msgs.reduce((sum, m) => sum + (m.tokenUsage?.inputTokens ?? 0), 0)

    return {
      iteration: iter,
      events,
      startedAt,
      endedAt,
      durationMs: endedAt - startedAt,
      totalTokens,
      totalInputTokens
    }
  })

  // Merge acceptance-gate-only iterations into previous iteration
  const processedIterations: IterationGroup[] = []
  for (const iter of iterations) {
    const hasSubstantiveEvents = iter.events.some((e) => e.type !== 'acceptance-gate')
    if (!hasSubstantiveEvents && processedIterations.length > 0) {
      const prev = processedIterations[processedIterations.length - 1]
      prev.events.push(...iter.events)
      prev.endedAt = Math.max(prev.endedAt, iter.endedAt)
      prev.durationMs = prev.endedAt - prev.startedAt
      prev.totalTokens += iter.totalTokens
      prev.totalInputTokens += iter.totalInputTokens
      continue
    }
    processedIterations.push(iter)
  }

  // Deduplicate consecutive acceptance-gate events, keeping the latest
  for (const iter of processedIterations) {
    const deduped: TimelineEvent[] = []
    for (const event of iter.events) {
      const prev = deduped[deduped.length - 1]
      if (prev && prev.type === 'acceptance-gate' && event.type === 'acceptance-gate') {
        deduped[deduped.length - 1] = event
      } else {
        deduped.push(event)
      }
    }
    iter.events = deduped
  }

  const mentorOutputTokens = sumTokensForRole(filtered, 'mentor', 'outputTokens')
  const mentorInputTokens = sumTokensForRole(filtered, 'mentor', 'inputTokens')
  const executorOutputTokens = sumTokensForRole(filtered, 'executor', 'outputTokens')
  const executorInputTokens = sumTokensForRole(filtered, 'executor', 'inputTokens')

  return {
    pairName: pair.name,
    spec: pair.spec,
    mentorModel: pair.mentorModel,
    executorModel: pair.executorModel,
    startedAt: pair.currentRunStartedAt,
    finishedAt: pair.currentRunFinishedAt,
    status: pair.status,
    iterations: processedIterations,
    totalOutputTokens: mentorOutputTokens + executorOutputTokens,
    totalInputTokens: mentorInputTokens + executorInputTokens,
    mentorOutputTokens,
    mentorInputTokens,
    executorOutputTokens,
    executorInputTokens,
    acceptanceRecords: pair.latestAcceptance ? [pair.latestAcceptance] : [],
    modifiedFiles: pair.modifiedFiles,
    durationMs: (pair.currentRunFinishedAt ?? Date.now()) - pair.currentRunStartedAt
  }
}

// ── Formatting utils ───────────────────────────────────

export function formatDuration(ms: number): string {
  if (ms <= 0) return '0s'
  const seconds = Math.floor(ms / 1000)
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`
  const hours = Math.floor(minutes / 60)
  return `${hours}h ${minutes % 60}m`
}

export function formatTokenCount(count: number): string {
  if (count >= 1000000) return `${(count / 1000000).toFixed(1)}M`
  if (count >= 1000) return `${(count / 1000).toFixed(1)}k`
  return count.toString()
}

export function formatTimestamp(ts: number): string {
  const d = new Date(ts)
  return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}:${d.getSeconds().toString().padStart(2, '0')}`
}

function formatDate(ts: number): string {
  const d = new Date(ts)
  return `${d.getFullYear()}-${(d.getMonth() + 1).toString().padStart(2, '0')}-${d.getDate().toString().padStart(2, '0')}`
}

export function formatDateTime(ts: number): string {
  return `${formatDate(ts)} ${formatTimestamp(ts)}`
}
