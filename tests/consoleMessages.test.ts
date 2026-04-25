import assert from 'node:assert/strict'
import test from 'node:test'

import { collapseConsecutiveConsoleMessages } from '../src/renderer/src/lib/consoleMessages.ts'

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
