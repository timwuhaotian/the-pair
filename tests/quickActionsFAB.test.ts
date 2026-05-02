import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const source = await readFile(
  new URL('../src/renderer/src/components/QuickActionsFAB.tsx', import.meta.url),
  'utf8'
)

test('QuickActionsFAB exports QuickActionsFAB component', () => {
  assert.match(source, /export function QuickActionsFAB\(/)
})

test('QuickActionsFAB uses framer-motion for animations', () => {
  assert.match(source, /framer-motion/)
})

test('QuickActionsFAB has fixed bottom-right positioning', () => {
  assert.match(source, /fixed/)
  assert.match(source, /bottom/)
  assert.match(source, /right/)
})

test('QuickActionsFAB menu items include create pair action', () => {
  assert.match(source, /[Cc]reate|onAction/)
})
