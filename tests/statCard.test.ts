import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const source = await readFile(
  new URL('../src/renderer/src/components/ui/StatCard.tsx', import.meta.url),
  'utf8'
)

test('StatCard exports StatCard component', () => {
  assert.match(source, /export function StatCard\(/)
})

test('StatCardProps interface defines value, label, and color', () => {
  assert.match(source, /interface StatCardProps/)
  assert.match(source, /value:\s*(number|string)\s*\|\s*(string|number)/)
  assert.match(source, /label:\s*string/)
  assert.match(source, /color:\s*['"]primary['"]\s*\|/)
})

test('StatCard uses cn utility for class merging', () => {
  assert.match(source, /import.*\bcn\b.*from/)
})

test('StatCard uses glass-card CSS class', () => {
  assert.match(source, /glass-card/)
})

test('StatCard colorMap is typed to StatCardProps color union', () => {
  assert.match(source, /StatCardProps\['color'\]/)
})

test('StatCard renders value and label in JSX', () => {
  assert.match(source, /\{value\}/)
  assert.match(source, /\{label\}/)
})
