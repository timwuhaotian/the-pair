import assert from 'node:assert/strict'
import test from 'node:test'

import type { AvailableModel } from '../src/renderer/src/types.ts'
import { buildProviderSetupSummary } from '../src/renderer/src/lib/providerSetup.ts'

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

const readyGeminiModel: AvailableModel = {
  provider: 'gemini',
  modelId: 'gemini-2.5-pro',
  displayName: 'Gemini 2.5 Pro',
  available: true,
  providerLabel: 'Antigravity',
  sourceProvider: 'google',
  sourceProviderLabel: 'Google',
  billingKind: 'plan',
  billingLabel: 'Included with plan',
  accessLabel: 'Google account',
  planLabel: 'individual',
  availabilityStatus: 'ready',
  supportsPairExecution: true,
  recommendedRoles: ['mentor', 'executor']
}

const blockedOpenCodeModel: AvailableModel = {
  provider: 'opencode',
  modelId: 'openai/gpt-4o-mini',
  displayName: 'GPT-4o Mini',
  available: false,
  providerLabel: 'OpenCode',
  sourceProvider: 'openai',
  sourceProviderLabel: 'OpenAI',
  billingKind: 'byok',
  billingLabel: 'Pay as you go',
  accessLabel: 'OpenAI API key',
  planLabel: 'provider-backed',
  availabilityStatus: 'auth-missing',
  availabilityReason: 'OpenCode is not signed in',
  supportsPairExecution: true,
  recommendedRoles: ['mentor', 'executor']
}

const readyAiderModel: AvailableModel = {
  provider: 'aider',
  modelId: 'claude-sonnet-4-6',
  displayName: 'Claude Sonnet 4.6 via Aider',
  available: true,
  providerLabel: 'Aider',
  sourceProvider: 'aider',
  sourceProviderLabel: 'Aider',
  billingKind: 'byok',
  billingLabel: 'Pay as you go',
  accessLabel: 'API key',
  planLabel: 'byok',
  availabilityStatus: 'ready',
  supportsPairExecution: true,
  recommendedRoles: ['mentor', 'executor']
}

test('buildProviderSetupSummary counts ready models from every supported provider', () => {
  const summary = buildProviderSetupSummary([
    blockedOpenCodeModel,
    readyClaudeModel,
    readyGeminiModel,
    readyAiderModel
  ])

  assert.equal(summary.isReady, true)
  assert.equal(summary.readyModelCount, 3)
  assert.deepEqual(summary.readyProviderLabels, ['Aider', 'Antigravity', 'Claude Code'])
})
