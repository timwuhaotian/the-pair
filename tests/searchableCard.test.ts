import assert from 'node:assert/strict'
import test from 'node:test'

import {
  countSearchHighlights,
  displayContentForMessage
} from '../src/renderer/src/lib/searchableCard.ts'
import type { Message } from '../src/renderer/src/store/usePairStore.ts'

function makeMessage(overrides: Partial<Message>): Message {
  return {
    id: 'm1',
    timestamp: 1000,
    from: 'mentor',
    to: 'executor',
    type: 'plan',
    content: 'parser plan',
    iteration: 1,
    ...overrides
  }
}

test('countSearchHighlights counts agent card matches on the rendered body', () => {
  assert.equal(countSearchHighlights(makeMessage({}), 'parser'), 1)
  assert.equal(countSearchHighlights(makeMessage({}), ' PARSER '), 1)
  assert.equal(countSearchHighlights(makeMessage({}), 'zzz'), 0)
})

test('countSearchHighlights returns 0 for an empty query', () => {
  assert.equal(countSearchHighlights(makeMessage({}), ''), 0)
  assert.equal(countSearchHighlights(makeMessage({}), '   '), 0)
})

test('countSearchHighlights excludes human banners (no anchor, no highlight)', () => {
  assert.equal(
    countSearchHighlights(makeMessage({ from: 'human', type: 'feedback', iteration: 0 }), 'parser'),
    0
  )
})

test('countSearchHighlights excludes handoff and empty bodies', () => {
  assert.equal(countSearchHighlights(makeMessage({ type: 'handoff' }), 'parser'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content: '  ' }), 'parser'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content: '{}' }), 'parser'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content: '[]' }), 'parser'), 0)
})

test('countSearchHighlights excludes technical handoffs except executor cards', () => {
  const content = '### ROLE: MENTOR\nparser instructions'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parser'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content, from: 'executor' }), 'parser'), 1)
})

test('countSearchHighlights excludes structured acceptance bodies', () => {
  const verdict = JSON.stringify({
    verdict: 'pass',
    risk: 'low',
    confidence: 0.9,
    issues: [],
    evidence: ['parser evidence'],
    summary: 'parser ok',
    nextStep: { action: 'finish', instructions: [] }
  })
  assert.equal(
    countSearchHighlights(makeMessage({ type: 'acceptance', content: verdict }), 'parser'),
    0
  )
  assert.equal(countSearchHighlights(makeMessage({ content: verdict }), 'parser'), 0)
})

test('displayContentForMessage strips system prompts like MessageCard', () => {
  assert.equal(displayContentForMessage(makeMessage({ content: '  parser plan  ' })), 'parser plan')
  assert.equal(
    displayContentForMessage(makeMessage({ from: 'human', type: 'feedback', content: '  parser  ' })),
    'parser'
  )
})
