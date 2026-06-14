import assert from 'node:assert/strict'
import test from 'node:test'

import type { AvailableModel } from '../src/renderer/src/types.ts'
import { buildProviderSetupHints } from '../src/renderer/src/lib/providerSetup.ts'

function makeModel(
  provider: AvailableModel['provider'],
  availabilityStatus: AvailableModel['availabilityStatus'],
  overrides: Partial<AvailableModel> = {}
): AvailableModel {
  return {
    provider,
    modelId: `${provider}-model`,
    displayName: `${provider} Model`,
    available: availabilityStatus === 'ready',
    providerLabel:
      provider === 'claude'
        ? 'Claude Code'
        : provider === 'codex'
          ? 'Codex'
          : provider === 'gemini'
            ? 'Gemini CLI'
            : 'OpenCode',
    sourceProvider: provider,
    sourceProviderLabel: provider,
    billingKind: 'plan',
    billingLabel: 'Included with plan',
    accessLabel: 'login',
    availabilityStatus,
    supportsPairExecution: true,
    recommendedRoles: ['mentor', 'executor'],
    ...overrides
  }
}

test('buildProviderSetupHints classifies ready providers', () => {
  const hints = buildProviderSetupHints([
    makeModel('claude', 'ready'),
    makeModel('codex', 'ready', { modelId: 'codex-1' }),
    makeModel('codex', 'ready', { modelId: 'codex-2' })
  ])

  assert.equal(hints.length, 2)

  const claude = hints.find((h) => h.kind === 'claude')
  assert.ok(claude)
  assert.equal(claude!.installed, true)
  assert.equal(claude!.authenticated, true)
  assert.equal(claude!.readyModelCount, 1)

  const codex = hints.find((h) => h.kind === 'codex')
  assert.ok(codex)
  assert.equal(codex!.readyModelCount, 2)
})

test('buildProviderSetupHints detects auth-missing providers', () => {
  const hints = buildProviderSetupHints([
    makeModel('claude', 'ready'),
    makeModel('codex', 'auth-missing')
  ])

  const codex = hints.find((h) => h.kind === 'codex')
  assert.ok(codex)
  assert.equal(codex!.installed, true, 'codex CLI is installed')
  assert.equal(codex!.authenticated, false, 'codex is not authenticated')
  assert.equal(codex!.readyModelCount, 0)
})

test('buildProviderSetupHints detects cli-missing providers', () => {
  const hints = buildProviderSetupHints([
    makeModel('claude', 'ready'),
    makeModel('gemini', 'cli-missing')
  ])

  const gemini = hints.find((h) => h.kind === 'gemini')
  assert.ok(gemini)
  assert.equal(gemini!.installed, false, 'gemini CLI is not installed')
  assert.equal(gemini!.authenticated, false)
  assert.equal(gemini!.readyModelCount, 0)
})

test('buildProviderSetupHints sorts ready before auth-missing before cli-missing', () => {
  const hints = buildProviderSetupHints([
    makeModel('opencode', 'cli-missing'),
    makeModel('codex', 'auth-missing'),
    makeModel('claude', 'ready'),
    makeModel('gemini', 'ready', { modelId: 'gemini-1' })
  ])

  assert.equal(hints[0].readyModelCount > 0, true, 'first hint should be ready')
  assert.equal(hints[1].readyModelCount > 0, true, 'second hint should be ready')
  assert.equal(hints[2].installed, true, 'third hint should be auth-missing (installed)')
  assert.equal(hints[2].readyModelCount, 0)
  assert.equal(hints[3].installed, false, 'fourth hint should be cli-missing')
})

test('buildProviderSetupHints includes login commands for known providers', () => {
  const hints = buildProviderSetupHints([
    makeModel('claude', 'ready'),
    makeModel('codex', 'ready')
  ])

  const claude = hints.find((h) => h.kind === 'claude')
  assert.equal(claude!.loginCommand, 'claude login')

  const codex = hints.find((h) => h.kind === 'codex')
  assert.equal(codex!.loginCommand, 'codex auth')
})

test('buildProviderSetupHints includes install URLs for known providers', () => {
  const hints = buildProviderSetupHints([
    makeModel('claude', 'cli-missing'),
    makeModel('opencode', 'cli-missing')
  ])

  const claude = hints.find((h) => h.kind === 'claude')
  assert.ok(claude!.installUrl)
  assert.ok(claude!.installUrl!.includes('claude'))

  const opencode = hints.find((h) => h.kind === 'opencode')
  assert.ok(opencode!.installUrl)
})

test('buildProviderSetupHints handles empty model list', () => {
  const hints = buildProviderSetupHints([])
  assert.equal(hints.length, 0)
})
