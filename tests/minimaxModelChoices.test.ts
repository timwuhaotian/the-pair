import assert from 'node:assert/strict'
import test from 'node:test'
import type { AvailableModel } from '../src/renderer/src/types.ts'
import {
  ordinaryModelChoices,
  preferMiniMaxFlash
} from '../src/renderer/src/lib/minimaxModelChoices.ts'
import {
  buildCanonicalModels,
  resolveSelection
} from '../src/renderer/src/lib/modelCatalogGrouping.ts'
import { getPreferredQualifiedModel } from '../src/renderer/src/lib/modelPreferences.ts'

function model(modelId: string, overrides: Partial<AvailableModel> = {}): AvailableModel {
  return {
    provider: 'opencode',
    modelId,
    displayName: modelId,
    available: true,
    providerLabel: 'OpenCode',
    sourceProviderLabel: 'MiniMax',
    billingKind: 'byok',
    billingLabel: 'Pay as you go',
    accessLabel: 'API key',
    planLabel: 'provider-backed',
    availabilityStatus: 'ready',
    supportsPairExecution: true,
    recommendedRoles: ['mentor', 'executor'],
    ...overrides
  }
}

const legacy = model('minimax/MiniMax-M2.7')
const m3 = model('minimax/MiniMax-M3')
const flash = model('minimax/MiniMax-M3.1-Flash')

test('ordinary MiniMax choices are Flash first, then M3, without mutating discovery', () => {
  const input = [legacy, m3, flash, model('minimax/MiniMax-Text-01')]
  assert.deepEqual(ordinaryModelChoices(input), [flash, m3])
  assert.equal(input[0], legacy)
  assert.equal(input.length, 4)
})

test('gateway IDs are preserved and legacy choices remain until a current model is discovered', () => {
  const routes = [
    model('opencode/minimax-m3'),
    model('opencode/minimax-m2.5-free'),
    model('opencode/minimax-m3.1-flash-free'),
    model('openrouter/minimax/minimax-m3.1-flash:free'),
    model('minimax-cn/MiniMax-M3')
  ]
  assert.deepEqual(ordinaryModelChoices(routes), [routes[2], routes[3], routes[0], routes[4]])
  assert.deepEqual(ordinaryModelChoices([legacy, m3]), [m3])
  assert.deepEqual(ordinaryModelChoices([legacy]), [legacy])
})

test('non-MiniMax positions, media, custom providers and aliases survive filtering', () => {
  const other = model('example/other-model')
  const media = model('minimax/image-01')
  const speech = model('minimax/speech-2.8-hd')
  const video = model('minimax/MiniMax-Hailuo-2.3')
  const custom = model(legacy.modelId, { planLabel: 'custom-provider' })
  const alias = model('company/my-minimax-deployment')
  const customPi = model('company/MiniMax-M2.7', { provider: 'pi', planLabel: 'pi-backed' })
  const nativeAlias = model('MiniMax-M2.7', { provider: 'aider' })
  assert.deepEqual(ordinaryModelChoices([customPi, nativeAlias, flash]), [
    customPi,
    nativeAlias,
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

test('canonical picker ordering is Flash first while legacy resolution retains its exact route', () => {
  const choices = preferMiniMaxFlash(
    buildCanonicalModels(ordinaryModelChoices([legacy, m3, flash])),
    (entry) => entry.routes[0].baseQualifiedId
  )
  assert.equal(choices[0].routes[0].baseQualifiedId, flash.modelId)
  const saved = resolveSelection(buildCanonicalModels([legacy, m3, flash]), legacy.modelId)
  assert.equal(saved.route?.baseQualifiedId, legacy.modelId)
})

test('new defaults prefer Flash, saved legacy/custom choices win, and unrelated defaults stay', () => {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
  let saved = ''
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: { getItem: () => saved }
  })
  try {
    assert.equal(getPreferredQualifiedModel('mentor', [legacy, m3, flash]), flash.modelId)
    assert.equal(getPreferredQualifiedModel('executor', [legacy, m3]), m3.modelId)
    assert.equal(getPreferredQualifiedModel('executor', [legacy]), legacy.modelId)
    const other = model('example/other-model')
    assert.equal(getPreferredQualifiedModel('mentor', [other, legacy, m3, flash]), other.modelId)
    assert.equal(getPreferredQualifiedModel('mentor', [legacy, other, m3, flash]), flash.modelId)
    assert.equal(getPreferredQualifiedModel('mentor', [legacy, other, m3]), m3.modelId)
    assert.equal(
      getPreferredQualifiedModel('mentor', [legacy, other, m3, { ...flash, available: false }]),
      m3.modelId
    )
    const customLegacy = model(legacy.modelId, { planLabel: 'custom-provider' })
    assert.equal(
      getPreferredQualifiedModel('mentor', [customLegacy, m3, flash]),
      customLegacy.modelId
    )
    saved = legacy.modelId
    assert.equal(getPreferredQualifiedModel('mentor', [legacy, m3, flash]), legacy.modelId)
    const custom = model('company/my-deployment', { planLabel: 'custom-provider' })
    saved = custom.modelId
    assert.equal(
      getPreferredQualifiedModel('executor', [legacy, m3, flash, custom]),
      custom.modelId
    )
    assert.equal(saved, custom.modelId)
  } finally {
    if (descriptor) Object.defineProperty(globalThis, 'localStorage', descriptor)
    else Reflect.deleteProperty(globalThis, 'localStorage')
  }
})
