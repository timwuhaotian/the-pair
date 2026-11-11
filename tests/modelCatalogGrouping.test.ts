import assert from 'node:assert/strict'
import test from 'node:test'

import type { ModelRoute, ProviderView } from '../src/renderer/src/types.ts'
import {
  buildCanonicalModels,
  defaultLeafForRoute,
  groupEndpoints,
  modelMatchesQuery,
  modelVendorKey,
  pickDefaultRoute,
  resolveSelection,
  saveLastRouteKey
} from '../src/renderer/src/lib/modelCatalogGrouping.ts'
import { vendorIconKey } from '../src/renderer/src/lib/brandIcons.ts'

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

const sonnet = route('claude-sonnet-4-5', { displayName: 'Claude Sonnet 4.5' })

const direct = provider('anthropic-direct', {
  vendor: 'anthropic',
  displayName: 'Anthropic',
  readyAccountCount: 1,
  models: [sonnet]
})

const keylessGateway = provider('anthropic-gateway', {
  vendor: 'anthropic',
  displayName: 'Anthropic Gateway',
  readyAccountCount: 0,
  models: [sonnet]
})

test('merges one model listed by several endpoints into a single canonical row', () => {
  const models = buildCanonicalModels([keylessGateway, direct])
  assert.equal(models.length, 1)
  assert.equal(models[0].canonicalKey, 'anthropic::claude-sonnet-4-5')
  assert.equal(models[0].displayName, 'Claude Sonnet 4.5')
  assert.equal(models[0].ready, true)
  assert.deepEqual(
    models[0].routes.map((route) => route.providerId),
    ['anthropic-direct', 'anthropic-gateway']
  )
})

test('a model only reachable through a keyless endpoint is not ready', () => {
  const models = buildCanonicalModels([keylessGateway])
  assert.equal(models[0].ready, false)
  assert.equal(models[0].routes[0].ready, false)
})

test('different vendors stay separate canonical models', () => {
  const grok = provider('xai', {
    vendor: 'xai',
    displayName: 'xAI',
    readyAccountCount: 1,
    models: [route('claude-sonnet-4-5', { displayName: 'Claude Sonnet 4.5' })]
  })
  const models = buildCanonicalModels([direct, grok])
  assert.deepEqual(models.map((model) => model.canonicalKey).sort(), [
    'anthropic::claude-sonnet-4-5',
    'xai::claude-sonnet-4-5'
  ])
})

test('resolveSelection matches a plain id and the legacy qualified spelling', () => {
  const models = buildCanonicalModels([direct])
  assert.equal(resolveSelection(models, 'claude-sonnet-4-5').route?.providerId, 'anthropic-direct')
  assert.equal(
    resolveSelection(models, 'claude/claude-sonnet-4-5').route?.providerId,
    'anthropic-direct'
  )
  assert.deepEqual(resolveSelection(models, 'unknown-model'), {})
})

test('resolveSelection reports the effort only when the endpoint lists it', () => {
  const codex = provider('openai', {
    vendor: 'openai',
    displayName: 'OpenAI',
    readyAccountCount: 1,
    models: [route('gpt-5', { efforts: ['low', 'medium', 'high'], defaultEffort: 'medium' })]
  })
  const models = buildCanonicalModels([codex])

  assert.deepEqual(resolveSelection(models, 'gpt-5', 'high').effort, { value: 'high' })
  // Value matches but the effort is unset: the route resolves, the effort does not.
  assert.equal(resolveSelection(models, 'gpt-5').route?.providerId, 'openai')
  assert.equal(resolveSelection(models, 'gpt-5').effort, undefined)
  // An effort the endpoint does not accept is not a selection either.
  assert.equal(resolveSelection(models, 'gpt-5', 'xhigh').effort, undefined)
})

test('defaultLeafForRoute takes the model default, then medium, then nothing', () => {
  const withDefault = buildCanonicalModels([
    provider('openai', {
      vendor: 'openai',
      models: [route('gpt-5', { efforts: ['low', 'medium', 'high'], defaultEffort: 'low' })]
    })
  ])[0].routes[0]
  assert.deepEqual(defaultLeafForRoute(withDefault), {
    modelId: 'gpt-5',
    reasoningEffort: 'low'
  })

  const noDefault = buildCanonicalModels([
    provider('openai', {
      vendor: 'openai',
      models: [route('gpt-5', { efforts: ['low', 'medium', 'high'] })]
    })
  ])[0].routes[0]
  assert.deepEqual(defaultLeafForRoute(noDefault), {
    modelId: 'gpt-5',
    reasoningEffort: 'medium'
  })

  const noEfforts = buildCanonicalModels([
    provider('openai', { vendor: 'openai', models: [sonnet] })
  ])[0].routes[0]
  assert.deepEqual(defaultLeafForRoute(noEfforts), {
    modelId: 'claude-sonnet-4-5',
    reasoningEffort: undefined
  })
})

const secondReady = provider('anthropic-secondary', {
  vendor: 'anthropic',
  displayName: 'Anthropic (secondary)',
  readyAccountCount: 1,
  models: [sonnet]
})

test('pickDefaultRoute remembers the last route per role and prefers a usable one', () => {
  const models = buildCanonicalModels([secondReady, direct])
  // Routes sort by label, so the primary endpoint leads.
  assert.deepEqual(
    models[0].routes.map((route) => route.providerId),
    ['anthropic-direct', 'anthropic-secondary']
  )

  const restore = installLocalStorage()
  try {
    // With nothing remembered, the first usable endpoint wins.
    assert.equal(pickDefaultRoute(models[0], 'mentor')?.providerId, 'anthropic-direct')

    // A remembered route for this role overrides the default order.
    saveLastRouteKey('mentor', models[0].canonicalKey, 'anthropic-secondary::claude-sonnet-4-5')
    assert.equal(pickDefaultRoute(models[0], 'mentor')?.providerId, 'anthropic-secondary')
    // A different role is unaffected.
    assert.equal(pickDefaultRoute(models[0], 'executor')?.providerId, 'anthropic-direct')
  } finally {
    restore()
  }
})

test('pickDefaultRoute ignores a remembered endpoint that has no ready account', () => {
  const models = buildCanonicalModels([keylessGateway, direct])

  const restore = installLocalStorage()
  try {
    saveLastRouteKey('mentor', models[0].canonicalKey, 'anthropic-gateway::claude-sonnet-4-5')
    assert.equal(pickDefaultRoute(models[0], 'mentor')?.providerId, 'anthropic-direct')
  } finally {
    restore()
  }
})

test('fuzzy search matches on model name, vendor and endpoint label', () => {
  const models = buildCanonicalModels([direct])
  assert.equal(modelMatchesQuery(models[0], 'sonnet45'), true)
  assert.equal(modelMatchesQuery(models[0], 'anthropic'), true)
  assert.equal(modelMatchesQuery(models[0], 'Anthropic'), true)
  assert.equal(modelMatchesQuery(models[0], 'zzz'), false)
})

test('groupEndpoints groups by vendor with ready endpoints first', () => {
  const openRouter = provider('openrouter', {
    vendor: 'openrouter',
    displayName: 'OpenRouter',
    readyAccountCount: 1,
    models: [route('claude-sonnet-4-5')]
  })
  const groups = groupEndpoints([keylessGateway, openRouter, direct])

  assert.deepEqual(
    groups.map((group) => [group.vendor, group.endpoints.map((e) => e.key)]),
    [
      ['anthropic', ['anthropic-direct', 'anthropic-gateway']],
      ['openrouter', ['openrouter']]
    ]
  )
  assert.deepEqual(
    groups[0].endpoints.map((endpoint) => endpoint.ready),
    [true, false]
  )
  assert.equal(groups[0].endpoints[0].models.length, 1)
})

test('reads the model vendor from the canonical key and maps it to a brand mark', () => {
  assert.equal(modelVendorKey('anthropic::claude-sonnet-4-5'), 'anthropic')
  assert.equal(vendorIconKey(modelVendorKey('anthropic::claude-sonnet-4-5')), 'claude')
  assert.equal(vendorIconKey('xai'), 'grok')
  // Gateway source labels arrive lower-cased with spaces or dots.
  assert.equal(vendorIconKey('github copilot'), 'githubcopilot')
  assert.equal(vendorIconKey('Z.ai'), 'zai')
  assert.equal(vendorIconKey('unknownvendor'), undefined)
})
