import assert from 'node:assert/strict'
import test from 'node:test'

import type { AvailableModel } from '../src/renderer/src/types.ts'
import {
  buildAgentConfig,
  getModelByQualifiedId,
  inferProviderFromModel,
  modelIdsEquivalent,
  resolveModelProvider
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
  // Aider routes via the aider/ qualifier or the keyword heuristic.
  assert.equal(inferProviderFromModel('aider/claude-sonnet-4-6'), 'aider')
  assert.equal(inferProviderFromModel('aider-sonnet'), 'aider')
  // Grok routes via the grok/ qualifier or the keyword heuristic.
  assert.equal(inferProviderFromModel('grok/grok-4.6'), 'grok')
  assert.equal(inferProviderFromModel('grok-4.6'), 'grok')
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

// ── getModelByQualifiedId ──────────────────────────────

function makeModel(
  provider: string,
  modelId: string,
  base: AvailableModel = readyOpenCodeModel
): AvailableModel {
  return { ...base, provider: provider as AvailableModel['provider'], modelId }
}

test('getModelByQualifiedId finds opencode model by bare id', () => {
  const models = [
    makeModel('opencode', 'glm-5-turbo'),
    makeModel('claude', 'claude-sonnet-4', readyClaudeModel),
    makeModel('kimi', 'wanqing/kat-coder-pro', readyClaudeModel)
  ]
  const result = getModelByQualifiedId(models, 'glm-5-turbo')
  assert.ok(result)
  assert.equal(result.provider, 'opencode')
  assert.equal(result.modelId, 'glm-5-turbo')
})

test('getModelByQualifiedId finds claude model by provider/modelId', () => {
  const models = [
    makeModel('opencode', 'glm-5-turbo'),
    makeModel('claude', 'claude-sonnet-4', readyClaudeModel),
    makeModel('kimi', 'wanqing/kat-coder-pro', readyClaudeModel)
  ]
  const result = getModelByQualifiedId(models, 'claude/claude-sonnet-4')
  assert.ok(result)
  assert.equal(result.provider, 'claude')
  assert.equal(result.modelId, 'claude-sonnet-4')
})

test('getModelByQualifiedId finds kimi model with slashed modelId', () => {
  const models = [
    makeModel('opencode', 'glm-5-turbo'),
    makeModel('claude', 'claude-sonnet-4', readyClaudeModel),
    makeModel('kimi', 'wanqing/kat-coder-pro', readyClaudeModel)
  ]
  const result = getModelByQualifiedId(models, 'kimi/wanqing/kat-coder-pro')
  assert.ok(result)
  assert.equal(result.provider, 'kimi')
  assert.equal(result.modelId, 'wanqing/kat-coder-pro')
})

test('getModelByQualifiedId returns undefined for nonexistent id', () => {
  const models = [
    makeModel('opencode', 'glm-5-turbo'),
    makeModel('claude', 'claude-sonnet-4', readyClaudeModel)
  ]
  const result = getModelByQualifiedId(models, 'nonexistent')
  assert.equal(result, undefined)
})

test('buildAgentConfig keeps the aider qualifier in the stored model id', () => {
  const readyAiderModel: AvailableModel = {
    ...readyClaudeModel,
    provider: 'aider',
    modelId: 'claude-sonnet-4-6',
    displayName: 'Claude Sonnet 4.6 via Aider',
    providerLabel: 'Aider',
    sourceProvider: 'aider',
    sourceProviderLabel: 'Aider',
    accessLabel: 'API key'
  }
  const config = buildAgentConfig('mentor', 'aider/claude-sonnet-4-6', [readyAiderModel])

  // The bare model id is not re-inferable as aider, so the qualifier must survive.
  assert.deepEqual(config, {
    role: 'mentor',
    provider: 'aider',
    model: 'aider/claude-sonnet-4-6'
  })
})

test('buildAgentConfig keeps the grok qualifier in the stored model id', () => {
  const readyGrokModel: AvailableModel = {
    ...readyClaudeModel,
    provider: 'grok',
    modelId: 'grok-4.6',
    displayName: 'Grok 4.6',
    providerLabel: 'Grok Build',
    sourceProvider: 'xai',
    sourceProviderLabel: 'xAI',
    billingKind: 'byok',
    billingLabel: 'Pay as you go',
    accessLabel: 'Grok Build login'
  }
  const config = buildAgentConfig('executor', 'grok/grok-4.6', [readyGrokModel])

  assert.deepEqual(config, {
    role: 'executor',
    provider: 'grok',
    model: 'grok/grok-4.6'
  })
  assert.equal(inferProviderFromModel(config.model), 'grok')

  // A custom `[model.<alias>]` alias has no grok keyword; only the qualifier routes it.
  const alias = buildAgentConfig('executor', 'grok/fast', [{ ...readyGrokModel, modelId: 'fast' }])
  assert.equal(alias.model, 'grok/fast')
  assert.equal(inferProviderFromModel(alias.model), 'grok')
})

const readyCodexModel: AvailableModel = {
  ...readyClaudeModel,
  provider: 'codex',
  modelId: 'codex-mini-latest',
  displayName: 'Codex Mini',
  providerLabel: 'Codex',
  sourceProvider: 'openai',
  sourceProviderLabel: 'OpenAI',
  accessLabel: 'Codex login'
}

test('buildAgentConfig keeps the codex qualifier so codex-* slugs never route to opencode', () => {
  const config = buildAgentConfig('mentor', 'codex/codex-mini-latest', [readyCodexModel])
  assert.deepEqual(config, {
    role: 'mentor',
    provider: 'codex',
    model: 'codex/codex-mini-latest'
  })
  assert.equal(inferProviderFromModel(config.model), 'codex')
})

test('buildAgentConfig still stores bare claude and gemini ids', () => {
  const gemini: AvailableModel = {
    ...readyClaudeModel,
    provider: 'gemini',
    modelId: 'gemini-2.5-pro'
  }
  assert.equal(
    buildAgentConfig('mentor', 'gemini/gemini-2.5-pro', [gemini]).model,
    'gemini-2.5-pro'
  )
  assert.equal(buildAgentConfig('mentor', 'claude/sonnet', [readyClaudeModel]).model, 'sonnet')
})

test('inferProviderFromModel is case-insensitive and routes the codex- prefix (Rust lockstep)', () => {
  // Antigravity display-name ids are capitalized.
  assert.equal(inferProviderFromModel('Gemini 3.5 Flash (Low)'), 'gemini')
  assert.equal(inferProviderFromModel('Claude-Opus-5'), 'claude')
  assert.equal(inferProviderFromModel('GPT-5.5'), 'codex')
  assert.equal(inferProviderFromModel('O3-mini'), 'codex')
  assert.equal(inferProviderFromModel('Codex/gpt-5'), 'codex')
  assert.equal(inferProviderFromModel('GROK/fast'), 'grok')
  assert.equal(inferProviderFromModel('codex-mini-latest'), 'codex')
  assert.equal(inferProviderFromModel('Codex-Mini-Latest'), 'codex')
  // Unknown slashed ids and plain aliases still fall back to opencode.
  assert.equal(inferProviderFromModel('anthropic/claude-sonnet-4'), 'opencode')
  assert.equal(inferProviderFromModel('opencode'), 'opencode')
  assert.equal(inferProviderFromModel('OpenCode'), 'opencode')
  assert.equal(inferProviderFromModel('fast'), 'opencode')
  // Keywords win over the codex- prefix (same order as Rust).
  assert.equal(inferProviderFromModel('codex-claude-bridge'), 'claude')
})

test('inferProviderFromModel mirrors the Rust infer_provider_kind cases', () => {
  assert.equal(inferProviderFromModel('codex-mini-latest'), 'codex')
  assert.equal(inferProviderFromModel('Gemini 3.5 Flash (Low)'), 'gemini')
  assert.equal(inferProviderFromModel('CODEX/gpt-5'), 'codex')
  assert.equal(inferProviderFromModel('grok/my-model'), 'grok')
})

test('getModelByQualifiedId accepts qualified ids and legacy bare ids', () => {
  const grok: AvailableModel = { ...readyClaudeModel, provider: 'grok', modelId: 'fast' }
  const models = [readyOpenCodeModel, readyClaudeModel, readyCodexModel, grok]

  assert.equal(getModelByQualifiedId(models, 'codex/codex-mini-latest'), readyCodexModel)
  // Ids stored by older builds (bare codex/grok/muse/claude/gemini ids).
  assert.equal(getModelByQualifiedId(models, 'codex-mini-latest'), readyCodexModel)
  assert.equal(getModelByQualifiedId(models, 'fast'), grok)
  assert.equal(getModelByQualifiedId(models, 'sonnet'), readyClaudeModel)
  // OpenCode ids are matched as-is and win over a same-named legacy bare id.
  assert.equal(getModelByQualifiedId(models, 'gpt-4o-mini'), readyOpenCodeModel)
  // Kimi/Pi/Kiro/Aider were always stored qualified: no bare fallback for them.
  const kimi: AvailableModel = { ...readyClaudeModel, provider: 'kimi', modelId: 'k3' }
  assert.equal(getModelByQualifiedId([kimi], 'k3'), undefined)
  assert.equal(getModelByQualifiedId([kimi], 'kimi/k3'), kimi)
})

test('getModelByQualifiedId prefers the inferred provider for an ambiguous bare id', () => {
  const codexGpt: AvailableModel = { ...readyCodexModel, modelId: 'gpt-5' }
  const museGpt: AvailableModel = { ...readyClaudeModel, provider: 'muse', modelId: 'gpt-5' }
  assert.equal(getModelByQualifiedId([museGpt, codexGpt], 'gpt-5'), codexGpt)
})

test('modelIdsEquivalent treats a qualified id and its legacy bare id as the same model', () => {
  assert.equal(modelIdsEquivalent('codex/gpt-5', 'gpt-5'), true)
  assert.equal(modelIdsEquivalent('gpt-5', 'codex/gpt-5'), true)
  assert.equal(modelIdsEquivalent('claude/sonnet', 'claude/sonnet'), true)
  assert.equal(modelIdsEquivalent('codex/gpt-5', 'claude/gpt-5'), false)
  assert.equal(modelIdsEquivalent('kimi/k3', 'k3'), false)
  assert.equal(modelIdsEquivalent('anthropic/claude-x', 'claude-x'), false)
})

test('resolveModelProvider keeps the known provider for an unchanged model', () => {
  const grok: AvailableModel = { ...readyClaudeModel, provider: 'grok', modelId: 'fast' }
  // Unchanged (even across qualified/bare spelling): the stored provider wins,
  // no catalog needed.
  assert.equal(resolveModelProvider([], 'fast', { modelId: 'grok/fast', provider: 'grok' }), 'grok')
  // Changed: the catalog decides…
  assert.equal(
    resolveModelProvider([grok], 'grok/fast', { modelId: 'claude/sonnet', provider: 'claude' }),
    'grok'
  )
  // …falling back to inference when the catalog doesn't list the model.
  assert.equal(
    resolveModelProvider([], 'codex/gpt-5', { modelId: 'claude/sonnet', provider: 'claude' }),
    'codex'
  )
})
