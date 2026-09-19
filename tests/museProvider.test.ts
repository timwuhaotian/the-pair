import assert from 'node:assert/strict'
import test from 'node:test'

import type { AvailableModel } from '../src/renderer/src/types.ts'
import {
  buildAgentConfig,
  inferProviderFromModel
} from '../src/renderer/src/lib/providerResolution.ts'
import { buildUpdateModelsPayload } from '../src/renderer/src/lib/modelResolution.ts'
import { PROVIDER_LOGIN_COMMANDS } from '../src/renderer/src/lib/providerSetup.ts'

const museModel: AvailableModel = {
  provider: 'muse',
  modelId: 'muse-spark-1.3',
  displayName: 'muse-spark-1.3',
  available: true,
  providerLabel: 'Muse Code',
  sourceProvider: 'meta',
  sourceProviderLabel: 'Meta',
  billingKind: 'plan',
  billingLabel: 'Meta account',
  accessLabel: 'Muse Code login',
  planLabel: 'plan-included',
  availabilityStatus: 'ready',
  supportsPairExecution: true,
  recommendedRoles: ['mentor', 'executor']
}

test('muse model ids infer the muse provider', () => {
  assert.equal(inferProviderFromModel('muse-spark-1.3'), 'muse')
  assert.equal(inferProviderFromModel('muse-spark-1.2'), 'muse')
  // Qualified form used by the model picker.
  assert.equal(inferProviderFromModel('muse/muse-spark-1.3'), 'muse')
})

test('muse inference does not steal other providers', () => {
  assert.equal(inferProviderFromModel('claude-opus-5'), 'claude')
  assert.equal(inferProviderFromModel('kimi/kimi-code/k3'), 'kimi')
  assert.equal(inferProviderFromModel('gpt-5'), 'codex')
})

test('muse models are stored bare, like claude and codex', () => {
  const config = buildAgentConfig('mentor', 'muse/muse-spark-1.3', [museModel])
  assert.deepEqual(config, {
    role: 'mentor',
    provider: 'muse',
    model: 'muse-spark-1.3'
  })
})

test('the muse qualifier is stripped from update payloads', () => {
  const payload = buildUpdateModelsPayload(
    { id: 'p1', mentorModel: 'muse-spark-1.3', executorModel: 'muse-spark-1.3' },
    {
      mentorModel: 'muse/muse-spark-1.3',
      executorModel: 'muse/muse-spark-1.2'
    }
  )
  assert.equal(payload.pendingMentorModel, 'muse-spark-1.3')
  assert.equal(payload.pendingExecutorModel, 'muse-spark-1.2')
})

test('muse exposes a login command for the onboarding screen', () => {
  assert.equal(PROVIDER_LOGIN_COMMANDS.muse, 'muse login')
})
