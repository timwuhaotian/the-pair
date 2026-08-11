import assert from 'node:assert/strict'
import test from 'node:test'

import type { AvailableModel } from '../src/renderer/src/types.ts'
import {
  buildCanonicalModels,
  defaultLeafForRoute,
  pickDefaultRoute,
  resolveSelection,
  saveLastRouteKey,
  modelMatchesQuery
} from '../src/renderer/src/lib/modelCatalogGrouping.ts'

function installLocalStorage(initial: Record<string, string> = {}): () => void {
  const store = new Map(Object.entries(initial))
  const storage = {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => {
      store.set(key, value)
    },
    removeItem: (key: string) => {
      store.delete(key)
    },
    clear: () => store.clear()
  }
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
  Object.defineProperty(globalThis, 'localStorage', {
    value: storage,
    configurable: true,
    writable: true
  })
  return () => {
    if (descriptor) Object.defineProperty(globalThis, 'localStorage', descriptor)
    else Reflect.deleteProperty(globalThis, 'localStorage')
  }
}

function makeModel(
  overrides: Partial<AvailableModel> & {
    provider: AvailableModel['provider']
    modelId: string
  }
): AvailableModel {
  return {
    displayName: overrides.modelId,
    available: true,
    providerLabel: overrides.provider,
    sourceProviderLabel: '',
    billingKind: 'plan',
    billingLabel: '',
    accessLabel: '',
    availabilityStatus: 'ready',
    supportsPairExecution: true,
    recommendedRoles: ['mentor', 'executor'],
    ...overrides
  } as AvailableModel
}

function antigravityFlash(effort: string): AvailableModel {
  const cap = effort.charAt(0).toUpperCase() + effort.slice(1)
  return makeModel({
    provider: 'gemini',
    modelId: `Gemini 3.5 Flash (${cap})`,
    displayName: `Gemini 3.5 Flash (${cap})`,
    providerLabel: 'Antigravity',
    accessLabel: 'Google account',
    planLabel: 'antigravity-backed',
    canonicalKey: 'google::gemini-3-5-flash',
    canonicalDisplayName: 'Gemini 3.5 Flash',
    effortTag: effort
  })
}

test('collapses baked-in effort variants into one model with one route and ordered efforts', () => {
  const models = buildCanonicalModels([
    antigravityFlash('high'),
    antigravityFlash('low'),
    antigravityFlash('medium')
  ])

  assert.equal(models.length, 1)
  assert.equal(models[0].displayName, 'Gemini 3.5 Flash')
  assert.equal(models[0].routes.length, 1)

  const route = models[0].routes[0]
  assert.deepEqual(
    route.effortOptions.map((option) => option.value),
    ['low', 'medium', 'high']
  )
  // every effort maps to its own distinct model id (effort is baked into the id)
  assert.equal(route.effortOptions[2].qualifiedId, 'gemini/Gemini 3.5 Flash (High)')
  assert.equal(route.effortOptions[2].reasoningEffort, undefined)
  // default leaf picks medium
  assert.equal(defaultLeafForRoute(route).qualifiedId, 'gemini/Gemini 3.5 Flash (Medium)')

  const sel = resolveSelection(models, 'gemini/Gemini 3.5 Flash (High)')
  assert.equal(sel.effort?.value, 'high')
  assert.equal(sel.route?.provider, 'gemini')
})

test('merges the same model across native + OpenCode routes under one canonical key', () => {
  const claude = makeModel({
    provider: 'claude',
    modelId: 'claude-sonnet-4-5-20250929',
    displayName: 'Claude Sonnet 4.5',
    providerLabel: 'Claude Code',
    accessLabel: 'Claude Code login',
    planLabel: 'subscription-backed',
    canonicalKey: 'anthropic::claude-sonnet-4-5',
    canonicalDisplayName: 'Claude Sonnet 4.5'
  })
  const opencode = makeModel({
    provider: 'opencode',
    modelId: 'anthropic/claude-sonnet-4-5',
    displayName: 'Claude Sonnet 4.5',
    providerLabel: 'OpenCode',
    accessLabel: 'Anthropic API key',
    planLabel: 'pay-as-you-go',
    billingKind: 'byok',
    canonicalKey: 'anthropic::claude-sonnet-4-5',
    canonicalDisplayName: 'Claude Sonnet 4.5'
  })

  const models = buildCanonicalModels([opencode, claude])
  assert.equal(models.length, 1)
  assert.equal(models[0].routes.length, 2)

  const restore = installLocalStorage()
  try {
    // with nothing remembered, the native plan route wins over pay-as-you-go OpenCode
    assert.equal(pickDefaultRoute(models[0], 'mentor')?.provider, 'claude')

    // a remembered route for this role overrides the preference order
    saveLastRouteKey('mentor', 'anthropic::claude-sonnet-4-5', 'opencode::pay-as-you-go')
    assert.equal(pickDefaultRoute(models[0], 'mentor')?.provider, 'opencode')
    // a different role is unaffected
    assert.equal(pickDefaultRoute(models[0], 'executor')?.provider, 'claude')
  } finally {
    restore()
  }
})

test('exposes Codex reasoning levels as a shared-id effort axis and resolves selection', () => {
  const codex = makeModel({
    provider: 'codex',
    modelId: 'o3',
    displayName: 'o3',
    providerLabel: 'Codex',
    accessLabel: 'ChatGPT plan',
    planLabel: 'subscription-backed',
    canonicalKey: 'openai::o3',
    canonicalDisplayName: 'o3',
    reasoningEffortLevels: ['low', 'medium', 'high']
  })

  const models = buildCanonicalModels([codex])
  const route = models[0].routes[0]
  assert.deepEqual(
    route.effortOptions.map((option) => option.reasoningEffort),
    ['low', 'medium', 'high']
  )
  assert.ok(route.effortOptions.every((option) => option.qualifiedId === 'codex/o3'))

  assert.equal(resolveSelection(models, 'codex/o3', 'high').effort?.value, 'high')
  assert.deepEqual(defaultLeafForRoute(route), {
    qualifiedId: 'codex/o3',
    reasoningEffort: 'medium'
  })
  // value matches but effort flag unset -> route resolves, effort undefined
  const unset = resolveSelection(models, 'codex/o3', undefined)
  assert.equal(unset.route?.provider, 'codex')
  assert.equal(unset.effort, undefined)
})

test('exposes OpenCode adaptive reasoning variants and resolves selection', () => {
  const model = makeModel({
    provider: 'opencode',
    modelId: 'minimax/MiniMax-M3',
    displayName: 'MiniMax-M3',
    providerLabel: 'OpenCode',
    accessLabel: 'MiniMax API key',
    planLabel: 'internal-provider',
    billingKind: 'byok',
    canonicalKey: 'minimax::minimax-m3',
    canonicalDisplayName: 'MiniMax-M3',
    reasoningEffortLevels: ['adaptive', 'disabled']
  })

  const models = buildCanonicalModels([model])
  const route = models[0].routes[0]
  assert.deepEqual(
    route.effortOptions.map((option) => option.reasoningEffort),
    ['adaptive', 'disabled']
  )
  assert.deepEqual(defaultLeafForRoute(route), {
    qualifiedId: 'minimax/MiniMax-M3',
    reasoningEffort: undefined
  })
  assert.equal(resolveSelection(models, 'minimax/MiniMax-M3', 'disabled').effort?.value, 'disabled')
})

test('fuzzy search matches on model name and route labels', () => {
  const models = buildCanonicalModels([antigravityFlash('low')])
  assert.equal(modelMatchesQuery(models[0], 'gem35flash'), true)
  assert.equal(modelMatchesQuery(models[0], 'google'), true)
  assert.equal(modelMatchesQuery(models[0], 'zzz'), false)
})

test('falls back to a derived canonical key when the backend omits one', () => {
  const a = makeModel({ provider: 'opencode', modelId: 'deepseek/deepseek-chat' })
  const models = buildCanonicalModels([a])
  assert.equal(models.length, 1)
  assert.equal(models[0].routes.length, 1)
  assert.equal(models[0].routes[0].baseQualifiedId, 'deepseek/deepseek-chat')
})
