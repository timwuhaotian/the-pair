import assert from 'node:assert/strict'
import test from 'node:test'

import {
  buildTimeline,
  formatDuration,
  formatTokenCount,
  formatTimestamp,
  formatDateTime,
  eventTitle,
  isTechnicalHandoff,
  type TimelineMessage
} from '../src/renderer/src/lib/timeline.ts'

// ── formatDuration ─────────────────────────────────────

test('formatDuration formats zero as 0s', () => {
  assert.equal(formatDuration(0), '0s')
})

test('formatDuration formats seconds', () => {
  assert.equal(formatDuration(5000), '5s')
})

test('formatDuration formats minutes and seconds', () => {
  assert.equal(formatDuration(65000), '1m 5s')
})

test('formatDuration formats hours and minutes', () => {
  assert.equal(formatDuration(3700000), '1h 1m')
})

// ── formatTokenCount ───────────────────────────────────

test('formatTokenCount formats small numbers as-is', () => {
  assert.equal(formatTokenCount(500), '500')
})

test('formatTokenCount formats thousands with k suffix', () => {
  assert.equal(formatTokenCount(1500), '1.5k')
})

test('formatTokenCount formats millions with M suffix', () => {
  assert.equal(formatTokenCount(2500000), '2.5M')
})

// ── formatTimestamp ────────────────────────────────────

test('formatTimestamp returns HH:MM:SS with zero-padding', () => {
  const ts = new Date(2024, 0, 1, 9, 5, 3).getTime()
  assert.equal(formatTimestamp(ts), '09:05:03')
})

// ── formatDateTime ─────────────────────────────────────

test('formatDateTime returns YYYY-MM-DD HH:MM:SS', () => {
  const ts = new Date(2024, 0, 5, 14, 30, 0).getTime()
  const result = formatDateTime(ts)
  assert.match(result, /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/)
  assert.ok(result.includes('2024-01-05'))
  assert.ok(result.includes('14:30:00'))
})

// ── isTechnicalHandoff ─────────────────────────────────

test('isTechnicalHandoff detects ROLE header', () => {
  assert.equal(isTechnicalHandoff('### ROLE: MENTOR'), true)
})

test('isTechnicalHandoff detects COMMAND TO EXECUTE', () => {
  assert.equal(isTechnicalHandoff('--- COMMAND TO EXECUTE ---'), true)
})

test('isTechnicalHandoff detects REVIEW REQUEST', () => {
  assert.equal(isTechnicalHandoff('--- REVIEW REQUEST ---'), true)
})

test('isTechnicalHandoff returns false for regular content', () => {
  assert.equal(isTechnicalHandoff('Here is my plan for the task'), false)
})

// ── eventTitle ─────────────────────────────────────────

test('eventTitle returns correct titles for each type', () => {
  assert.equal(eventTitle('mentor-plan'), 'Mentor Plan')
  assert.equal(eventTitle('executor-result'), 'Executor Execution')
  assert.equal(eventTitle('mentor-review'), 'Mentor Review')
  assert.equal(eventTitle('human-feedback'), 'Human Feedback')
  assert.equal(eventTitle('acceptance-gate'), 'Acceptance Gate')
  assert.equal(eventTitle('handoff'), 'Handoff')
})

// ── buildTimeline ──────────────────────────────────────

function makePair(overrides?: Partial<Record<string, unknown>>) {
  return {
    name: 'Test Pair',
    spec: 'Fix the bug',
    mentorModel: 'claude/sonnet',
    executorModel: 'gpt-4o',
    status: 'Finished',
    messages: [] as TimelineMessage[],
    modifiedFiles: [],
    currentRunStartedAt: 1000,
    currentRunFinishedAt: 5000,
    ...overrides
  }
}

test('buildTimeline groups mentor plan and executor result into one iteration', () => {
  const messages: TimelineMessage[] = [
    {
      id: 'm1',
      timestamp: 1000,
      from: 'mentor',
      to: 'executor',
      type: 'plan',
      content: 'Here is the plan',
      iteration: 1,
      tokenUsage: {
        outputTokens: 100,
        inputTokens: 50,
        lastUpdatedAt: 1000,
        source: 'live',
        provider: 'claude'
      }
    },
    {
      id: 'm2',
      timestamp: 2000,
      from: 'executor',
      to: 'mentor',
      type: 'result',
      content: 'Done implementing',
      iteration: 1,
      tokenUsage: {
        outputTokens: 200,
        inputTokens: 80,
        lastUpdatedAt: 2000,
        source: 'live',
        provider: 'codex'
      }
    }
  ]

  const timeline = buildTimeline(messages, makePair({ messages }))

  assert.equal(timeline.iterations.length, 1)
  assert.equal(timeline.iterations[0].events.length, 2)
  assert.equal(timeline.iterations[0].events[0].type, 'mentor-plan')
  assert.equal(timeline.iterations[0].events[1].type, 'executor-result')
  assert.equal(timeline.totalOutputTokens, 300)
  assert.equal(timeline.totalInputTokens, 130)
})

test('buildTimeline filters handoff messages', () => {
  const messages: TimelineMessage[] = [
    {
      id: 'm1',
      timestamp: 1000,
      from: 'mentor',
      to: 'executor',
      type: 'plan',
      content: 'Plan content',
      iteration: 1
    },
    {
      id: 'm2',
      timestamp: 2000,
      from: 'mentor',
      to: 'executor',
      type: 'handoff',
      content: 'Handing off to executor',
      iteration: 1
    }
  ]

  const timeline = buildTimeline(messages, makePair({ messages }))
  assert.equal(timeline.iterations[0].events.length, 1)
  assert.equal(timeline.iterations[0].events[0].type, 'mentor-plan')
})

test('buildTimeline filters empty content ({})', () => {
  const messages: TimelineMessage[] = [
    {
      id: 'm1',
      timestamp: 1000,
      from: 'mentor',
      to: 'executor',
      type: 'plan',
      content: '{}',
      iteration: 1
    },
    {
      id: 'm2',
      timestamp: 2000,
      from: 'executor',
      to: 'mentor',
      type: 'result',
      content: 'Real result',
      iteration: 1
    }
  ]

  const timeline = buildTimeline(messages, makePair({ messages }))
  assert.equal(timeline.iterations[0].events.length, 1)
})

test('buildTimeline filters empty content ([])', () => {
  const messages: TimelineMessage[] = [
    {
      id: 'm1',
      timestamp: 1000,
      from: 'mentor',
      to: 'executor',
      type: 'plan',
      content: '[]',
      iteration: 1
    },
    {
      id: 'm2',
      timestamp: 2000,
      from: 'executor',
      to: 'mentor',
      type: 'result',
      content: 'Real result',
      iteration: 1
    }
  ]

  const timeline = buildTimeline(messages, makePair({ messages }))
  assert.equal(timeline.iterations[0].events.length, 1)
})

test('buildTimeline creates separate groups for multiple iterations', () => {
  const messages: TimelineMessage[] = [
    {
      id: 'm1',
      timestamp: 1000,
      from: 'mentor',
      to: 'executor',
      type: 'plan',
      content: 'Iteration 1 plan',
      iteration: 1
    },
    {
      id: 'm2',
      timestamp: 3000,
      from: 'mentor',
      to: 'executor',
      type: 'plan',
      content: 'Iteration 2 plan',
      iteration: 2
    }
  ]

  const timeline = buildTimeline(messages, makePair({ messages }))
  assert.equal(timeline.iterations.length, 2)
  assert.equal(timeline.iterations[0].iteration, 1)
  assert.equal(timeline.iterations[1].iteration, 2)
})

test('buildTimeline computes durationMs for events', () => {
  const messages: TimelineMessage[] = [
    {
      id: 'm1',
      timestamp: 1000,
      from: 'mentor',
      to: 'executor',
      type: 'plan',
      content: 'First event',
      iteration: 1
    },
    {
      id: 'm2',
      timestamp: 3500,
      from: 'executor',
      to: 'mentor',
      type: 'result',
      content: 'Second event',
      iteration: 1
    }
  ]

  const timeline = buildTimeline(messages, makePair({ messages }))
  const events = timeline.iterations[0].events
  assert.equal(events[0].durationMs, 2500)
  // Last event has no next event → undefined
  assert.equal(events[1].durationMs, undefined)
})
