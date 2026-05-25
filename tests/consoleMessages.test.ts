import assert from 'node:assert/strict'
import test from 'node:test'

import {
  collapseConsecutiveConsoleMessages,
  collapseWithDropCounts
} from '../src/renderer/src/lib/consoleMessages.ts'

const baseMessage = {
  timestamp: 1000,
  to: 'human',
  iteration: 1
} as const

test('collapseConsecutiveConsoleMessages keeps the latest same-role message of the same type', () => {
  const messages = [
    {
      ...baseMessage,
      id: 'mentor-placeholder',
      from: 'mentor',
      type: 'plan',
      content: 'Waiting for first output...'
    },
    {
      ...baseMessage,
      id: 'mentor-final',
      from: 'mentor',
      type: 'plan',
      content: 'Based on my analysis, remove the unused imports.'
    }
  ]

  const collapsed = collapseConsecutiveConsoleMessages(messages)

  assert.deepEqual(
    collapsed.map((msg) => msg.id),
    ['mentor-final']
  )
})

test('collapseConsecutiveConsoleMessages does not hide a mentor acceptance from a later iteration', () => {
  const messages = [
    {
      ...baseMessage,
      id: 'mentor-plan',
      from: 'mentor',
      type: 'plan',
      content: 'Review the implementation.'
    },
    {
      ...baseMessage,
      iteration: 2,
      id: 'mentor-acceptance',
      from: 'mentor',
      type: 'acceptance',
      content: '{"verdict":"pass"}'
    }
  ]

  const collapsed = collapseConsecutiveConsoleMessages(messages)

  assert.deepEqual(
    collapsed.map((msg) => msg.id),
    ['mentor-plan', 'mentor-acceptance']
  )
})

test('collapseWithDropCounts reports how many same-role same-iteration messages were dropped before the survivor', () => {
  const messages = [
    {
      ...baseMessage,
      id: 'mentor-1',
      from: 'mentor',
      type: 'plan',
      content: 'first thought'
    },
    {
      ...baseMessage,
      id: 'mentor-2',
      from: 'mentor',
      type: 'plan',
      content: 'second thought'
    },
    {
      ...baseMessage,
      id: 'mentor-3',
      from: 'mentor',
      type: 'plan',
      content: 'final plan'
    }
  ]

  const { kept, droppedBeforeId } = collapseWithDropCounts(messages)

  assert.deepEqual(
    kept.map((m) => m.id),
    ['mentor-3']
  )
  assert.equal(droppedBeforeId.get('mentor-3'), 2)
})

test('collapseWithDropCounts reports zero drops when nothing was collapsed', () => {
  const messages = [
    {
      ...baseMessage,
      id: 'mentor-only',
      from: 'mentor',
      type: 'plan',
      content: 'only plan'
    }
  ]

  const { kept, droppedBeforeId } = collapseWithDropCounts(messages)

  assert.deepEqual(
    kept.map((m) => m.id),
    ['mentor-only']
  )
  assert.equal(droppedBeforeId.size, 0)
})

test('collapseConsecutiveConsoleMessages keeps only the final role summary per iteration', () => {
  const messages = [
    {
      ...baseMessage,
      id: 'mentor-plan',
      from: 'mentor',
      type: 'plan',
      content: 'Starting step'
    },
    {
      ...baseMessage,
      id: 'mentor-acceptance',
      from: 'mentor',
      type: 'acceptance',
      content: '{"verdict":"pass","summary":"All 3 greetings received"}'
    },
    {
      ...baseMessage,
      id: 'executor-progress',
      from: 'executor',
      type: 'progress',
      content: 'Working...'
    },
    {
      ...baseMessage,
      id: 'executor-result',
      from: 'executor',
      type: 'result',
      content: 'Greeting 3/3 received. TASK_COMPLETE'
    }
  ]

  const collapsed = collapseConsecutiveConsoleMessages(messages)

  assert.deepEqual(
    collapsed.map((msg) => msg.id),
    ['mentor-acceptance', 'executor-result']
  )
})
