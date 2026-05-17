import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const pairListGroup = await readFile(
  new URL('../src/renderer/src/components/PairListGroup.tsx', import.meta.url),
  'utf8'
)

const pairListSection = await readFile(
  new URL('../src/renderer/src/components/PairListSection.tsx', import.meta.url),
  'utf8'
)

test('PairListGroup exports PairListGroup component', () => {
  assert.match(pairListGroup, /export function PairListGroup\(/)
})

test('PairListGroup accepts title and pairs props', () => {
  assert.match(pairListGroup, /title/)
  assert.match(pairListGroup, /pairs.*Pair/)
})

test('PairListGroup renders pair items with onSelect callback', () => {
  assert.match(pairListGroup, /onSelectPair/)
})

test('PairListGroup highlights the currently selected pair', () => {
  assert.match(pairListGroup, /selectedPairId/)
  assert.match(pairListGroup, /aria-current/)
})

test('PairListSection exports PairListSection component', () => {
  assert.match(pairListSection, /export function PairListSection\(/)
})

test('PairListSection groups pairs by workspace then status', () => {
  assert.match(pairListSection, /buildWorkspaceGroups/)
  assert.match(pairListSection, /workspace\.statusGroups/)
})

test('PairListSection owns the create-pair call to action in the pairs area', () => {
  assert.match(pairListSection, /onCreatePair/)
  assert.match(pairListSection, /<Plus/)
  assert.match(pairListSection, /quickActions\.create/)
})
