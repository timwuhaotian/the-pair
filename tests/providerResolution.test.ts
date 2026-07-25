import assert from 'node:assert/strict'
import test from 'node:test'

import type { AvailableModel } from '../src/renderer/src/types.ts'
import {
  buildAgentConfig,
  inferProviderFromModel
} from '../src/renderer/src/lib/providerResolution.ts'

const readyOpenCodeModel: AvailableModel = {
  provider: 'opencode',
  modelId: 'gpt-4o-mini',
  displayName: 'GPT-4o Mini',
  available: true,
  providerLabel: 'OpenCode',
  sourceProvider: 'openai',
  sourceProviderLabel: 'OpenAI',
  billingKind: 'byok',
  billingLabel: 'Pay as you go',
  accessLabel: 'OpenAI API key',
  planLabel: 'provider-backed',
  availabilityStatus: 'ready',
  supportsPairExecution: true,
  recommendedRoles: ['mentor', 'executor']
}

const readyClaudeModel: AvailableModel = {
  provider: 'claude',
  modelId: 'sonnet',
  displayName: 'Claude Sonnet',
  available: true,
  providerLabel: 'Claude Code',
  sourceProvider: 'anthropic',
  sourceProviderLabel: 'Anthropic',
  billingKind: 'plan',
  billingLabel: 'Included with plan',
  accessLabel: 'Claude Code login',
  planLabel: 'pro',
  availabilityStatus: 'ready',
  supportsPairExecution: true,
  recommendedRoles: ['mentor', 'executor']
}

test('inferProviderFromModel maps provider-aware ids and legacy model names', () => {
  assert.equal(inferProviderFromModel('codex/gpt-4o-mini'), 'codex')
  assert.equal(inferProviderFromModel('claude-3-5-sonnet'), 'claude')
  assert.equal(inferProviderFromModel('gemini-2.5-pro'), 'gemini')
  assert.equal(inferProviderFromModel('gpt-4o-mini'), 'codex')
  // Kimi aliases contain their own slashes; the leading qualifier routes them.
  assert.equal(inferProviderFromModel('kimi/kimi-code/k3'), 'kimi')
  assert.equal(inferProviderFromModel('kimi-k2.5'), 'kimi')
  // Without the qualifier an arbitrary alias falls back to opencode — this is
  // why buildAgentConfig stores kimi ids qualified.
  assert.equal(inferProviderFromModel('ark-coding-plan/glm-5.2'), 'opencode')
})

test('buildAgentConfig preserves the selected provider and raw model id', () => {
  const config = buildAgentConfig('mentor', 'claude/sonnet', [readyClaudeModel, readyOpenCodeModel])

  assert.deepEqual(config, {
    role: 'mentor',
    provider: 'claude',
    model: 'sonnet'
  })
})

test('buildAgentConfig keeps the kimi qualifier in the stored model id', () => {
  const readyKimiModel: AvailableModel = {
    ...readyClaudeModel,
    provider: 'kimi',
    modelId: 'ark-coding-plan/glm-5.2',
    displayName: 'GLM-5.2 (Ark)',
    providerLabel: 'Kimi Code',
    sourceProvider: 'kimi',
    sourceProviderLabel: 'Kimi',
    accessLabel: 'Kimi Code login'
  }
  const config = buildAgentConfig('executor', 'kimi/ark-coding-plan/glm-5.2', [readyKimiModel])

  // The alias alone is not re-inferable as kimi, so the qualifier must survive.
  assert.deepEqual(config, {
    role: 'executor',
    provider: 'kimi',
    model: 'kimi/ark-coding-plan/glm-5.2'
  })
})
