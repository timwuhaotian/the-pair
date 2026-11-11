import assert from 'node:assert/strict'
import test from 'node:test'

import type { ModelRoute, ModelSource, ProviderView } from '../src/renderer/src/types.ts'
import {
  isOrdinaryModelChoice,
  ordinaryModelChoices,
  preferMiniMaxFlash,
  type ModelChoice
} from '../src/renderer/src/lib/minimaxModelChoices.ts'
import {
  buildCanonicalModels,
  resolveSelection
} from '../src/renderer/src/lib/modelCatalogGrouping.ts'
import { resolvePreferredModel } from '../src/renderer/src/lib/modelPreferences.ts'

function choice(id: string, source: ModelSource = 'imported'): ModelChoice {
  return { id, source }
}

function route(id: string, source: ModelSource = 'imported'): ModelRoute {
  return { id, displayName: id, efforts: [], source }
}

function provider(id: string, models: ModelRoute[], ready = true): ProviderView {
  return {
    id,
    protocol: 'openaiChat',
    baseUrl: `https://${id}.example.com`,
    vendor: 'minimax',
    displayName: id,
    accounts: [],
    models,
    readyAccountCount: ready ? 1 : 0
  }
}

const legacy = choice('minimax/MiniMax-M2.7')
const m3 = choice('minimax/MiniMax-M3')
const flash = choice('minimax/MiniMax-M3.1-Flash')

test('ordinary MiniMax choices are Flash first, then M3, without mutating discovery', () => {
  const input = [legacy, m3, flash, choice('minimax/MiniMax-Text-01')]
  assert.deepEqual(ordinaryModelChoices(input), [flash, m3])
  assert.equal(input[0], legacy)
  assert.equal(input.length, 4)
})

test('gateway IDs are preserved and legacy choices remain until a current model is discovered', () => {
  const routes = [
    choice('opencode/minimax-m3'),
    choice('opencode/minimax-m2.5-free'),
    choice('opencode/minimax-m3.1-flash-free'),
    choice('openrouter/minimax/minimax-m3.1-flash:free'),
    choice('minimax-cn/MiniMax-M3')
  ]
  assert.deepEqual(ordinaryModelChoices(routes), [routes[2], routes[3], routes[0], routes[4]])
  assert.deepEqual(ordinaryModelChoices([legacy, m3]), [m3])
  assert.deepEqual(ordinaryModelChoices([legacy]), [legacy])
})

test('non-MiniMax positions, media, hand-added providers and aliases survive filtering', () => {
  const other = choice('example/other-model')
  const media = choice('minimax/image-01')
  const speech = choice('minimax/speech-2.8-hd')
  const video = choice('minimax/MiniMax-Hailuo-2.3')
  const custom = choice(legacy.id, 'manual')
  const alias = choice('company/my-minimax-deployment')
  const importedAlias = choice('company/MiniMax-M2.7')
  const presetAlias = choice('MiniMax-M2.7', 'preset')
  assert.deepEqual(ordinaryModelChoices([importedAlias, presetAlias, flash]), [
    importedAlias,
    presetAlias,
    flash
  ])
  assert.deepEqual(ordinaryModelChoices([other, m3, media, flash, speech, video, custom, alias]), [
    other,
    flash,
    media,
    m3,
    speech,
    video,
    custom,
    alias
  ])
})

test('isOrdinaryModelChoice only curates rows imported from an agent config', () => {
  assert.equal(isOrdinaryModelChoice(choice('minimax/MiniMax-M2.7')), false)
  assert.equal(isOrdinaryModelChoice(choice('minimax/MiniMax-M3')), true)
  assert.equal(isOrdinaryModelChoice(choice('minimax/MiniMax-M2.7', 'manual')), true)
  assert.equal(isOrdinaryModelChoice(choice('minimax/MiniMax-M2.7', 'preset')), true)
})

test('canonical picker ordering is Flash first while legacy resolution retains its exact route', () => {
  const routes = [route(legacy.id), route(m3.id), route(flash.id)]
  const choices = preferMiniMaxFlash(
    buildCanonicalModels([provider('minimax', ordinaryModelChoices(routes))]),
    (entry) => entry.routes[0].modelId
  )
  assert.equal(choices[0].routes[0].modelId, flash.id)
  const saved = resolveSelection(buildCanonicalModels([provider('minimax', routes)]), legacy.id)
  assert.equal(saved.route?.modelId, legacy.id)
})

test('new defaults prefer Flash, saved legacy/custom choices win, and unrelated defaults stay', () => {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
  let saved = ''
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: { getItem: () => saved }
  })
  try {
    const registry = (models: ModelRoute[]): ProviderView[] => [provider('minimax', models)]
    const routes = (): ModelRoute[] => [route(legacy.id), route(m3.id), route(flash.id)]

    assert.equal(resolvePreferredModel('mentor', registry(routes())), flash.id)
    assert.equal(
      resolvePreferredModel('executor', registry([route(legacy.id), route(m3.id)])),
      m3.id
    )
    assert.equal(resolvePreferredModel('executor', registry([route(legacy.id)])), legacy.id)
    const other = route('example/other-model')
    assert.equal(resolvePreferredModel('mentor', registry([other, ...routes()])), other.id)
    assert.equal(
      resolvePreferredModel(
        'mentor',
        registry([route(legacy.id), other, route(m3.id), route(flash.id)])
      ),
      flash.id
    )
    assert.equal(
      resolvePreferredModel('mentor', registry([route(legacy.id), other, route(m3.id)])),
      m3.id
    )
    // A model on a keyless endpoint is not selectable, so it cannot be the default.
    assert.equal(
      resolvePreferredModel('mentor', [
        provider('keyless', [route(flash.id)], false),
        provider('minimax', [route(legacy.id), route(m3.id)])
      ]),
      m3.id
    )
    const customLegacy = route(legacy.id, 'manual')
    assert.equal(
      resolvePreferredModel('mentor', registry([customLegacy, route(m3.id), route(flash.id)])),
      customLegacy.id
    )
    saved = legacy.id
    assert.equal(resolvePreferredModel('mentor', registry(routes())), legacy.id)
    const custom = route('company/my-deployment', 'manual')
    saved = custom.id
    assert.equal(resolvePreferredModel('executor', registry([...routes(), custom])), custom.id)
    assert.equal(saved, custom.id)
  } finally {
    if (descriptor) Object.defineProperty(globalThis, 'localStorage', descriptor)
    else Reflect.deleteProperty(globalThis, 'localStorage')
  }
})
