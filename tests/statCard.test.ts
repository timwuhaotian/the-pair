import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const source = await readFile(
  new URL('../src/renderer/src/components/StatCard.tsx', import.meta.url),
  'utf8'
)

test('StatCard exports StatCard component', () => {
  assert.match(source, /export (function StatCard|const StatCard)/)
})

test('StatCard accepts value, label, and color props', () => {
  assert.match(source, /value/)
  assert.match(source, /label/)
  assert.match(source, /color/)
})

test('StatCard uses cn for class merging', () => {
  assert.match(source, /from ['"]\.\.\/.*utils['"]/)
  assert.match(source, /\bcn\b/)
})

test('StatCard uses glass-card base styling', () => {
  assert.match(source, /glass-card/)
})
