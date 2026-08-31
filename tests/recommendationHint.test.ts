import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const createPairModal = await readFile(
  new URL('../src/renderer/src/components/CreatePairModal.tsx', import.meta.url),
  'utf8'
)

test('CreatePairModal debounces recommendation lookups for the draft spec', () => {
  assert.match(createPairModal, /getRecommendation/)
  assert.match(createPairModal, /setTimeout/)
  assert.match(createPairModal, /clearTimeout/)
  assert.match(createPairModal, /\[spec\]/)
})

test('CreatePairModal hides the recommendation hint when there is no recommendation', () => {
  assert.match(createPairModal, /recommendation && \(/)
  assert.match(createPairModal, /setRecommendation\(null\)/)
})

test('CreatePairModal renders the hint line under the task spec field', () => {
  assert.match(createPairModal, /data-testid="recommendation-hint"/)
  assert.match(createPairModal, /modals\.recommendationHint/)
  assert.match(createPairModal, /recommendation\.mentorModel/)
  assert.match(createPairModal, /recommendation\.executorModel/)
  assert.match(createPairModal, /recommendation\.successes/)
  assert.match(createPairModal, /recommendation\.runs/)
})
