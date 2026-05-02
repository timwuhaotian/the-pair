import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const source = await readFile(
  new URL('../src/renderer/src/components/EmptyStateGuide.tsx', import.meta.url),
  'utf8'
)

test('EmptyStateGuide exports EmptyStateGuide component', () => {
  assert.match(source, /export function EmptyStateGuide\(/)
})

test('EmptyStateGuideProps interface defines onCreatePair callback', () => {
  assert.match(source, /interface EmptyStateGuideProps/)
  assert.match(source, /onCreatePair/)
})

test('EmptyStateGuide includes step-by-step guide (4 steps)', () => {
  assert.match(source, /step1|step2|step3|step4/i)
  const stepMatches = source.match(/step[1-4]/gi)
  if (!stepMatches || stepMatches.length < 4) {
    throw new Error(`Expected at least 4 step references, found ${stepMatches?.length || 0}`)
  }
})

test('EmptyStateGuide uses GlassButton for primary action', () => {
  assert.match(source, /GlassButton/)
})
