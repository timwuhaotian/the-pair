import assert from 'node:assert/strict'
import test from 'node:test'

import type { ModelRoute, ProviderView } from '../src/renderer/src/types.ts'
import {
  getModelByQualifiedId,
  modelIdsEquivalent,
  modelMatchesId
} from '../src/renderer/src/lib/providerResolution.ts'

function route(id: string, overrides: Partial<ModelRoute> = {}): ModelRoute {
  return { id, displayName: id, efforts: [], source: 'preset', ...overrides }
}

function provider(id: string, overrides: Partial<ProviderView> = {}): ProviderView {
  return {
    id,
    protocol: 'anthropic',
    baseUrl: `https://${id}.example.com`,
    vendor: id,
    displayName: id,
    accounts: [],
    models: [],
    readyAccountCount: 0,
    ...overrides
  }
}

const anthropic = provider('anthropic', {
  vendor: 'anthropic',
  displayName: 'Anthropic',
  readyAccountCount: 1,
  models: [route('claude-sonnet-4-5', { displayName: 'Claude Sonnet 4.5' })]
})

const openRouter = provider('openrouter', {
  vendor: 'openrouter',
  displayName: 'OpenRouter',
  models: [route('anthropic/claude-sonnet-4-5')]
})

// ── modelIdsEquivalent ──────────────────────────────

test('modelIdsEquivalent treats a qualified id and its legacy bare id as the same model', () => {
  assert.equal(modelIdsEquivalent('codex/gpt-5', 'gpt-5'), true)
  assert.equal(modelIdsEquivalent('gpt-5', 'codex/gpt-5'), true)
  assert.equal(modelIdsEquivalent('claude:sonnet', 'sonnet'), true)
  assert.equal(modelIdsEquivalent('claude/sonnet', 'claude/sonnet'), true)
  // Two differently-qualified ids never collapse: the qualifier is part of the id.
  assert.equal(modelIdsEquivalent('codex/gpt-5', 'claude/gpt-5'), false)
  // Gateway path segments are how a gateway names a model, not a stale qualifier.
  assert.equal(modelIdsEquivalent('anthropic/claude-sonnet-4-5', 'claude-sonnet-4-5'), false)
  assert.equal(modelIdsEquivalent('minimax-m3.1-flash:free', 'free'), false)
})

// ── modelMatchesId ──────────────────────────────

test('modelMatchesId accepts the registry id and the legacy qualified spelling', () => {
  const sonnet = route('claude-sonnet-4-5')
  assert.equal(modelMatchesId(sonnet, 'claude-sonnet-4-5'), true)
  assert.equal(modelMatchesId(sonnet, 'claude/claude-sonnet-4-5'), true)
  assert.equal(modelMatchesId(sonnet, 'claude-opus-5'), false)
})

// ── getModelByQualifiedId ──────────────────────────────

test('getModelByQualifiedId resolves plain ids, qualified ids and prefers ready endpoints', () => {
  const ready = provider('anthropic-direct', {
    vendor: 'anthropic',
    readyAccountCount: 1,
    models: [route('gpt-5', { displayName: 'GPT-5 (direct)' })]
  })
  const keyless = provider('anthropic-gateway', {
    vendor: 'anthropic',
    readyAccountCount: 0,
    models: [route('gpt-5', { displayName: 'GPT-5 (gateway)' })]
  })

  assert.equal(getModelByQualifiedId([keyless, ready], 'gpt-5')?.displayName, 'GPT-5 (direct)')
  assert.equal(
    getModelByQualifiedId([keyless, ready], 'codex/gpt-5')?.displayName,
    'GPT-5 (direct)'
  )
  assert.equal(
    getModelByQualifiedId([openRouter], 'anthropic/claude-sonnet-4-5')?.id,
    'anthropic/claude-sonnet-4-5'
  )
  assert.equal(getModelByQualifiedId([anthropic], 'nope'), undefined)
})
