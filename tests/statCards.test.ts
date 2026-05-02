import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const source = await readFile(
  new URL('../src/renderer/src/components/StatCards.tsx', import.meta.url),
  'utf8'
)

test('StatCards exports StatCards component', () => {
  assert.match(source, /export function StatCards\(/)
})

test('StatCardsProps interface defines total/running/paused/finished', () => {
  assert.match(source, /interface StatCardsProps/)
  assert.match(source, /total:\s*number/)
  assert.match(source, /running:\s*number/)
  assert.match(source, /paused:\s*number/)
  assert.match(source, /finished:\s*number/)
})

test('StatCards renders 4 StatCard children', () => {
  assert.match(source, /<StatCard/)
  const statCardMatches = source.match(/<StatCard/g)
  if (!statCardMatches || statCardMatches.length < 4) {
    throw new Error(`Expected 4 <StatCard> elements, found ${statCardMatches?.length || 0}`)
  }
})

test('StatCards uses responsive grid layout', () => {
  assert.match(source, /grid/)
  assert.match(source, /md:grid-cols-4/)
})
