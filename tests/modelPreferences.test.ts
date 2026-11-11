import assert from 'node:assert/strict'
import test from 'node:test'

import type { ModelRoute, ProviderView } from '../src/renderer/src/types.ts'
import {
  getPreferredModelId,
  isSelectableForPairExecution,
  resolvePreferredModel,
  savePreferredModelId
} from '../src/renderer/src/lib/modelPreferences.ts'

type LocalStorageLike = {
  getItem: (key: string) => string | null
  setItem: (key: string, value: string) => void
  removeItem: (key: string) => void
  clear: () => void
}

function installLocalStorage(initial: Record<string, string> = {}): () => void {
  const store = new Map(Object.entries(initial))
  const storage: LocalStorageLike = {
    getItem: (key) => store.get(key) ?? null,
    setItem: (key, value) => {
      store.set(key, value)
    },
    removeItem: (key) => {
      store.delete(key)
    },
    clear: () => {
      store.clear()
    }
  }

  const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
  Object.defineProperty(globalThis, 'localStorage', {
    value: storage,
    configurable: true,
    writable: true
  })

  return () => {
    if (descriptor) {
      Object.defineProperty(globalThis, 'localStorage', descriptor)
    } else {
      Reflect.deleteProperty(globalThis, 'localStorage')
    }
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

const anthropic = provider('anthropic', {
  vendor: 'anthropic',
  displayName: 'Anthropic',
  readyAccountCount: 1,
  models: [route('claude-sonnet-4-5', { displayName: 'Claude Sonnet 4.5' })]
})

const keyless = provider('keyless', {
  vendor: 'keyless',
  displayName: 'Keyless',
  readyAccountCount: 0,
  models: [route('claude-opus-5')]
})

test('savePreferredModelId and getPreferredModelId round-trip through localStorage', () => {
  const restore = installLocalStorage()
  try {
    savePreferredModelId('mentor', 'claude-sonnet-4-5')
    assert.equal(getPreferredModelId('mentor'), 'claude-sonnet-4-5')
  } finally {
    restore()
  }
})

test('isSelectableForPairExecution needs a ready account and something to run', () => {
  assert.equal(isSelectableForPairExecution(anthropic), true)
  assert.equal(isSelectableForPairExecution(keyless), false)
  assert.equal(isSelectableForPairExecution(provider('empty', { readyAccountCount: 1 })), false)
})

test('resolvePreferredModel returns the saved choice when the registry still offers it', () => {
  const restore = installLocalStorage({ 'the-pair-preferred-mentor-model': 'claude-sonnet-4-5' })
  try {
    assert.equal(resolvePreferredModel('mentor', [anthropic]), 'claude-sonnet-4-5')
  } finally {
    restore()
  }
})

test('resolvePreferredModel falls back to the first default choice when the saved one is gone', () => {
  const restore = installLocalStorage({ 'the-pair-preferred-executor-model': 'claude-opus-5' })
  try {
    assert.equal(resolvePreferredModel('executor', [keyless, anthropic]), 'claude-sonnet-4-5')
  } finally {
    restore()
  }
})

test('resolvePreferredModel resolves a legacy qualified preference and rewrites it plain', () => {
  const restore = installLocalStorage({
    'the-pair-preferred-mentor-model': 'claude/claude-sonnet-4-5'
  })
  try {
    assert.equal(resolvePreferredModel('mentor', [anthropic]), 'claude-sonnet-4-5')
    assert.equal(getPreferredModelId('mentor'), 'claude-sonnet-4-5')
  } finally {
    restore()
  }
})

test('resolvePreferredModel skips keyless endpoints even when they appear first', () => {
  const restore = installLocalStorage()
  try {
    assert.equal(resolvePreferredModel('mentor', [keyless, anthropic]), 'claude-sonnet-4-5')
  } finally {
    restore()
  }
})

test('resolvePreferredModel returns empty string when no endpoint can run a model', () => {
  const restore = installLocalStorage()
  try {
    assert.equal(resolvePreferredModel('executor', [keyless]), '')
  } finally {
    restore()
  }
})
