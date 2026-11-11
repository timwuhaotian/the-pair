import assert from 'node:assert/strict'
import test from 'node:test'

import type { ProviderView } from '../src/renderer/src/types.ts'
import { getModelByQualifiedId } from '../src/renderer/src/lib/providerResolution.ts'
import { buildUpdateModelsPayload } from '../src/renderer/src/lib/modelResolution.ts'

const museEndpoint: ProviderView = {
  id: 'muse',
  protocol: 'anthropic',
  baseUrl: 'https://muse.example.com',
  vendor: 'meta',
  displayName: 'Muse Code',
  accounts: [],
  models: [
    {
      id: 'muse-spark-1.3',
      displayName: 'muse-spark-1.3',
      efforts: [],
      source: 'imported'
    },
    { id: 'llama-5-coder', displayName: 'llama-5-coder', efforts: [], source: 'imported' }
  ],
  readyAccountCount: 1
}

test('a legacy muse-qualified id resolves to the registry spelling', () => {
  assert.equal(getModelByQualifiedId([museEndpoint], 'muse/muse-spark-1.3')?.id, 'muse-spark-1.3')
  // A settings.json model with no "muse" keyword resolves the same way.
  assert.equal(getModelByQualifiedId([museEndpoint], 'muse/llama-5-coder')?.id, 'llama-5-coder')
})

test('the legacy muse qualifier is normalised in update payloads', () => {
  const payload = buildUpdateModelsPayload(
    { mentorModel: 'muse-spark-1.3', executorModel: 'muse-spark-1.3' },
    {
      mentorModel: 'muse/muse-spark-1.3',
      executorModel: 'muse/llama-5-coder'
    }
  )
  assert.equal(payload.pendingMentorModel, 'muse-spark-1.3')
  assert.equal(payload.pendingExecutorModel, 'llama-5-coder')
})
