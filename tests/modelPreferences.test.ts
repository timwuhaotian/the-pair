import assert from 'node:assert/strict'
import test from 'node:test'

import type { AvailableModel } from '../src/renderer/src/types.ts'
import {
  getPreferredPairModelSelection,
  getPreferredModelId,
  getPreferredQualifiedModel,
  getQualifiedModel,
  isSelectableForPairExecution,
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

const unavailableClaudeModel: AvailableModel = {
  provider: 'claude',
  modelId: 'sonnet',
  displayName: 'Claude Sonnet',
  available: false,
  providerLabel: 'Claude Code',
  sourceProvider: 'anthropic',
  sourceProviderLabel: 'Anthropic',
  billingKind: 'plan',
  billingLabel: 'Included with plan',
  accessLabel: 'Claude Code login',
  planLabel: 'pro',
  availabilityStatus: 'runtime-unsupported',
  availabilityReason: 'Claude Code is detected, but pair execution is not yet supported',
  supportsPairExecution: false,
  recommendedRoles: ['mentor', 'executor']
}

test('getQualifiedModel keeps OpenCode ids unprefixed and prefixes other providers', () => {
  assert.equal(getQualifiedModel(readyOpenCodeModel), 'gpt-4o-mini')
  assert.equal(getQualifiedModel(unavailableClaudeModel), 'claude/sonnet')
})

test('savePreferredModelId and getPreferredModelId round-trip through localStorage', () => {
  const restore = installLocalStorage()
  try {
    savePreferredModelId('mentor', 'gpt-4o-mini')
    assert.equal(getPreferredModelId('mentor'), 'gpt-4o-mini')
  } finally {
    restore()
  }
})

test('getPreferredQualifiedModel falls back to the first available model when the saved choice is unavailable', () => {
  const restore = installLocalStorage({
    'the-pair-preferred-executor-model': 'claude/sonnet'
  })

  try {
    assert.equal(
      getPreferredQualifiedModel('executor', [unavailableClaudeModel, readyOpenCodeModel]),
      'gpt-4o-mini'
    )
  } finally {
    restore()
  }
})

const viewOnlyGeminiModel: AvailableModel = {
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
  planLabel: 'Individual',
  availabilityStatus: 'runtime-unsupported',
  availabilityReason: 'Antigravity is detected, but pair execution is not yet supported',
  supportsPairExecution: false,
  recommendedRoles: ['mentor', 'executor']
}

test('isSelectableForPairExecution returns false for view-only models', () => {
  assert.equal(isSelectableForPairExecution(readyOpenCodeModel), true)
  assert.equal(isSelectableForPairExecution(unavailableClaudeModel), false)
  assert.equal(isSelectableForPairExecution(viewOnlyGeminiModel), false)
})

test('getPreferredQualifiedModel skips view-only models even when they appear first', () => {
  const restore = installLocalStorage()
  try {
    const models = [viewOnlyGeminiModel, readyOpenCodeModel]
    assert.equal(getPreferredQualifiedModel('mentor', models), 'gpt-4o-mini')
  } finally {
    restore()
  }
})

test('getPreferredQualifiedModel ignores saved view-only preference and falls back to selectable', () => {
  const restore = installLocalStorage({
    'the-pair-preferred-mentor-model': 'gemini/gemini-2.5-pro'
  })

  try {
    const models = [viewOnlyGeminiModel, readyOpenCodeModel]
    assert.equal(getPreferredQualifiedModel('mentor', models), 'gpt-4o-mini')
  } finally {
    restore()
  }
})

test('getPreferredQualifiedModel returns empty string when no selectable models exist', () => {
  const restore = installLocalStorage()
  try {
    const models = [viewOnlyGeminiModel, unavailableClaudeModel]
    assert.equal(getPreferredQualifiedModel('executor', models), '')
  } finally {
    restore()
  }
})

test('getPreferredPairModelSelection returns the saved mentor and executor defaults', () => {
  const restore = installLocalStorage({
    'the-pair-preferred-mentor-model': 'gpt-4o-mini',
    'the-pair-preferred-executor-model': 'claude/sonnet'
  })

  try {
    const selection = getPreferredPairModelSelection([readyOpenCodeModel, unavailableClaudeModel])
    assert.deepEqual(selection, {
      mentorModel: 'gpt-4o-mini',
      executorModel: 'gpt-4o-mini'
    })
  } finally {
    restore()
  }
})
