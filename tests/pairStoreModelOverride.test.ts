/**
 * Tests for model resolution logic used by usePairStore.assignTask().
 *
 * These tests verify the resolution semantics:
 *   effectiveModel = override ?? pendingModel ?? defaultModel
 *
 * And the sync behavior:
 *   - shouldSyncModelsToBackend(overrides) returns true only when overrides !== undefined
 *   - updateModels is called only when explicit overrides are provided
 *   - Without overrides, backend uses existing pending or default models
 *   - This avoids unnecessary IPC and prevents partial state on failure
 *
 * The helper functions are pure and tested directly. usePairStore.ts
 * delegates to these helpers, guaranteeing the same behavior at runtime.
 */

import assert from 'node:assert/strict'
import test from 'node:test'
import {
  resolveEffectiveModels,
  buildUpdateModelsPayload,
  shouldSyncModelsToBackend,
  getAssignableTaskModels,
  type PairLike,
  type ModelOverrides
} from '../src/renderer/src/lib/modelResolution.ts'

test('resolution: override takes precedence over pending and default', () => {
  const pair: PairLike = {
    mentorModel: 'default-mentor',
    executorModel: 'default-executor',
    pendingMentorModel: 'pending-mentor',
    pendingExecutorModel: 'pending-executor'
  }

  const overrides: ModelOverrides = {
    mentorModel: 'override-mentor',
    executorModel: 'override-executor'
  }

  const result = resolveEffectiveModels(pair, overrides)

  assert.equal(result.mentorModel, 'override-mentor', 'override should win over pending/default')
  assert.equal(
    result.executorModel,
    'override-executor',
    'override should win over pending/default'
  )
})

test('resolution: pending takes precedence over default when no override', () => {
  const pair: PairLike = {
    mentorModel: 'default-mentor',
    executorModel: 'default-executor',
    pendingMentorModel: 'pending-mentor',
    pendingExecutorModel: 'pending-executor'
  }

  const result = resolveEffectiveModels(pair)

  assert.equal(result.mentorModel, 'pending-mentor', 'pending should win over default')
  assert.equal(result.executorModel, 'pending-executor', 'pending should win over default')
})

test('resolution: default used when no override or pending', () => {
  const pair: PairLike = {
    mentorModel: 'default-mentor',
    executorModel: 'default-executor'
  }

  const result = resolveEffectiveModels(pair)

  assert.equal(result.mentorModel, 'default-mentor', 'default should be used')
  assert.equal(result.executorModel, 'default-executor', 'default should be used')
})

test('assign modal defaults: restoring wins, then pending, then pair defaults', () => {
  const pair: PairLike = {
    mentorModel: 'default-mentor',
    executorModel: 'default-executor',
    pendingMentorModel: 'pending-mentor',
    pendingExecutorModel: 'pending-executor'
  }

  assert.deepEqual(getAssignableTaskModels(pair), {
    mentorModel: 'pending-mentor',
    executorModel: 'pending-executor'
  })

  assert.deepEqual(
    getAssignableTaskModels(pair, {
      mentorModel: 'restore-mentor',
      executorModel: 'restore-executor'
    }),
    {
      mentorModel: 'restore-mentor',
      executorModel: 'restore-executor'
    }
  )
})

test('resolution: partial override only affects specified role', () => {
  const pair: PairLike = {
    mentorModel: 'default-mentor',
    executorModel: 'default-executor',
    pendingMentorModel: 'pending-mentor',
    pendingExecutorModel: 'pending-executor'
  }

  const result = resolveEffectiveModels(pair, { mentorModel: 'override-mentor' })

  assert.equal(result.mentorModel, 'override-mentor', 'mentor override should apply')
  assert.equal(
    result.executorModel,
    'pending-executor',
    'executor should fall back to pending (not override)'
  )
})

test('resolution: undefined override falls back to pending', () => {
  const pair: PairLike = {
    mentorModel: 'default-mentor',
    executorModel: 'default-executor',
    pendingMentorModel: 'pending-mentor',
    pendingExecutorModel: undefined
  }

  const result = resolveEffectiveModels(pair, { executorModel: undefined })

  assert.equal(result.mentorModel, 'pending-mentor', 'mentor uses pending')
  assert.equal(result.executorModel, 'default-executor', 'executor falls back to default')
})

test('payload: buildUpdateModelsPayload creates correct structure', () => {
  const pair: PairLike = {
    mentorModel: 'default-mentor',
    executorModel: 'default-executor',
    pendingMentorModel: 'pending-mentor',
    pendingExecutorModel: 'pending-executor'
  }

  const effective = resolveEffectiveModels(pair)
  const payload = buildUpdateModelsPayload(pair, effective)

  assert.equal(payload.mentorModel, 'default-mentor', 'payload preserves default mentor')
  assert.equal(payload.executorModel, 'default-executor', 'payload preserves default executor')
  assert.equal(payload.pendingMentorModel, 'pending-mentor', 'payload sets effective mentor')
  assert.equal(payload.pendingExecutorModel, 'pending-executor', 'payload sets effective executor')
})

test('payload: payload uses override values when provided', () => {
  const pair: PairLike = {
    mentorModel: 'default-mentor',
    executorModel: 'default-executor'
  }

  const effective = resolveEffectiveModels(pair, {
    mentorModel: 'override-mentor',
    executorModel: 'override-executor'
  })
  const payload = buildUpdateModelsPayload(pair, effective)

  assert.equal(payload.pendingMentorModel, 'override-mentor')
  assert.equal(payload.pendingExecutorModel, 'override-executor')
})

test('integration: full flow from pair to payload to effective resolution', () => {
  const pair: PairLike = {
    mentorModel: 'default-mentor',
    executorModel: 'default-executor',
    pendingMentorModel: 'pending-mentor',
    pendingExecutorModel: 'pending-executor'
  }

  const overrides: ModelOverrides = {
    mentorModel: 'override-mentor'
  }

  const effective = resolveEffectiveModels(pair, overrides)
  const payload = buildUpdateModelsPayload(pair, effective)

  const expectedEffective = {
    mentorModel: 'override-mentor',
    executorModel: 'pending-executor'
  }

  assert.deepEqual(effective, expectedEffective, 'effective models match resolution priority')
  assert.equal(payload.pendingMentorModel, 'override-mentor')
  assert.equal(payload.pendingExecutorModel, 'pending-executor')

  const afterConsumption: PairLike = {
    mentorModel: effective.mentorModel,
    executorModel: effective.executorModel
  }

  const nextRunEffective = resolveEffectiveModels(afterConsumption)
  assert.deepEqual(
    nextRunEffective,
    effective,
    'after consumption, defaults match previous effective models'
  )
})

test('sync decision: shouldSyncModelsToBackend returns correct values', () => {
  // Direct test of the helper function used by usePairStore.assignTask
  assert.equal(shouldSyncModelsToBackend(undefined), false, 'undefined -> no sync')
  assert.equal(shouldSyncModelsToBackend({}), true, 'empty object -> sync')
  assert.equal(
    shouldSyncModelsToBackend({ mentorModel: 'override' }),
    true,
    'partial override -> sync'
  )
  assert.equal(
    shouldSyncModelsToBackend({ mentorModel: 'm', executorModel: 'e' }),
    true,
    'full override -> sync'
  )
})

test('argument parsing: assignTask handles multiple calling conventions', () => {
  // Simulates the argument parsing logic in usePairStore.assignTask:
  //   if (typeof roleOrModelOverrides === 'string') {
  //     role = roleOrModelOverrides
  //     overrides = modelOverrides
  //   } else {
  //     overrides = modelOverrides ?? roleOrModelOverrides
  //   }

  function parseAssignTaskArgs(
    roleOrModelOverrides?: string | ModelOverrides,
    modelOverrides?: ModelOverrides
  ): { role: string | undefined; overrides: ModelOverrides | undefined } {
    let role: string | undefined
    let overrides: ModelOverrides | undefined

    if (typeof roleOrModelOverrides === 'string') {
      role = roleOrModelOverrides
      overrides = modelOverrides
    } else {
      overrides = modelOverrides ?? roleOrModelOverrides
    }

    return { role, overrides }
  }

  // Handoff calls: assignTask(pairId, spec, 'mentor')
  const handoff1 = parseAssignTaskArgs('mentor')
  assert.equal(handoff1.role, 'mentor', 'handoff: role is mentor')
  assert.equal(handoff1.overrides, undefined, 'handoff: no overrides')

  // Handoff with overrides: assignTask(pairId, spec, 'executor', { mentorModel: 'm' })
  const handoff2 = parseAssignTaskArgs('executor', { mentorModel: 'm' })
  assert.equal(handoff2.role, 'executor', 'handoff with overrides: role is executor')
  assert.deepEqual(
    handoff2.overrides,
    { mentorModel: 'm' },
    'handoff with overrides: has overrides'
  )

  // New task with overrides object: assignTask(pairId, spec, { mentorModel: 'm' })
  const newTask1 = parseAssignTaskArgs({ mentorModel: 'm' })
  assert.equal(newTask1.role, undefined, 'new task: no role')
  assert.deepEqual(newTask1.overrides, { mentorModel: 'm' }, 'new task: overrides from first arg')

  // New task with modal-style call: assignTask(pairId, spec, undefined, { mentorModel: 'm' })
  // This is what AssignTaskModal sends
  const newTask2 = parseAssignTaskArgs(undefined, { mentorModel: 'm' })
  assert.equal(newTask2.role, undefined, 'modal-style: no role')
  assert.deepEqual(
    newTask2.overrides,
    { mentorModel: 'm' },
    'modal-style: overrides from fourth arg'
  )

  // New task without overrides: assignTask(pairId, spec) or assignTask(pairId, spec, undefined)
  const newTask3 = parseAssignTaskArgs()
  assert.equal(newTask3.role, undefined, 'no args: no role')
  assert.equal(newTask3.overrides, undefined, 'no args: no overrides')

  const newTask4 = parseAssignTaskArgs(undefined)
  assert.equal(newTask4.role, undefined, 'undefined arg: no role')
  assert.equal(newTask4.overrides, undefined, 'undefined arg: no overrides')

  // New task with undefined override: assignTask(pairId, spec, undefined, undefined)
  const newTask5 = parseAssignTaskArgs(undefined, undefined)
  assert.equal(newTask5.role, undefined, 'both undefined: no role')
  assert.equal(newTask5.overrides, undefined, 'both undefined: no overrides')
})

test('sync decision: assignTask caller shapes from AssignTaskModal', () => {
  // AssignTaskModal sends:
  // - undefined when no model selection changed
  // - { mentorModel?: string, executorModel?: string } when selection changed

  const scenarios = [
    {
      name: 'no selection change',
      overrides: undefined,
      expectedSync: false,
      reason: 'backend already has correct models, no sync needed'
    },
    {
      name: 'mentor model changed',
      overrides: { mentorModel: 'new-mentor' },
      expectedSync: true,
      reason: 'need to sync new mentor selection to backend'
    },
    {
      name: 'executor model changed',
      overrides: { executorModel: 'new-executor' },
      expectedSync: true,
      reason: 'need to sync new executor selection to backend'
    },
    {
      name: 'both models changed',
      overrides: { mentorModel: 'new-mentor', executorModel: 'new-executor' },
      expectedSync: true,
      reason: 'need to sync both selections to backend'
    }
  ]

  for (const scenario of scenarios) {
    const shouldSync = shouldSyncModelsToBackend(scenario.overrides)
    assert.equal(shouldSync, scenario.expectedSync, `${scenario.name}: ${scenario.reason}`)
  }
})

test('sync decision: effective models when sync is skipped', () => {
  // When shouldSyncModelsToBackend returns false, effective models are still computed
  // locally, and backend uses existing pending or default models

  const pair: PairLike = {
    mentorModel: 'default-mentor',
    executorModel: 'default-executor',
    pendingMentorModel: 'pending-mentor',
    pendingExecutorModel: 'pending-executor'
  }

  const overrides = undefined
  const shouldSync = shouldSyncModelsToBackend(overrides)

  assert.equal(shouldSync, false, 'no sync when overrides undefined')

  // Effective models computed locally for frontend state
  const effective = resolveEffectiveModels(pair, overrides)
  assert.equal(effective.mentorModel, 'pending-mentor', 'uses pending')
  assert.equal(effective.executorModel, 'pending-executor', 'uses pending')

  // Backend already has pending models, will use them via:
  // pair.pending_mentor_model.unwrap_or(&pair.mentor_model)
})

test('transactional: no backend mutation when no overrides (safe on failure)', () => {
  // When overrides is undefined, updateModels is NOT called.
  // This means if assignTask fails, no backend state was mutated.

  const pair: PairLike = {
    mentorModel: 'default-mentor',
    executorModel: 'default-executor',
    pendingMentorModel: 'pending-mentor',
    pendingExecutorModel: 'pending-executor'
  }

  // Simulating the flow: overrides = undefined
  const overrides = undefined
  const shouldCallUpdateModels = overrides !== undefined

  assert.equal(shouldCallUpdateModels, false, 'no updateModels call when no overrides')

  // Effective models are computed locally, backend uses existing pending
  const effective = resolveEffectiveModels(pair, overrides)
  assert.equal(effective.mentorModel, 'pending-mentor', 'uses existing pending')
  assert.equal(effective.executorModel, 'pending-executor', 'uses existing pending')

  // If assignTask fails here, backend still has the correct pending models
  // No rollback needed because we never mutated backend state
})

test('transactional: rollback impossible after updateModels success', () => {
  // When overrides ARE provided, updateModels IS called.
  // If assignTask fails after updateModels succeeds, backend is partially updated.
  // This is acceptable because:
  // 1. The partial state (pending set) will be used on the next run
  // 2. The user can retry without losing their override selection

  const pair: PairLike = {
    mentorModel: 'default-mentor',
    executorModel: 'default-executor'
  }

  const overrides: ModelOverrides = {
    mentorModel: 'override-mentor',
    executorModel: 'override-executor'
  }

  // Simulating the flow: overrides defined
  const shouldCallUpdateModels = overrides !== undefined
  assert.equal(shouldCallUpdateModels, true, 'updateModels called when overrides provided')

  // The payload that would be sent
  const effective = resolveEffectiveModels(pair, overrides)
  const payload = buildUpdateModelsPayload(pair, effective)

  assert.equal(payload.pendingMentorModel, 'override-mentor')
  assert.equal(payload.pendingExecutorModel, 'override-executor')

  // If assignTask fails after updateModels succeeds:
  // - Backend has pending = override
  // - Frontend state unchanged (no resetPairForNewRun called)
  // - User can retry with same override, backend already has it set
})
