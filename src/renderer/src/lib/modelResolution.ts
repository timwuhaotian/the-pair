import type { PairModelSelection } from '../types'
import { modelIdsEquivalent, stripModelQualifier } from './providerResolution'

export type PairLike = {
  mentorModel: string
  executorModel: string
  pendingMentorModel?: string
  pendingExecutorModel?: string
  mentorReasoningEffort?: string
  executorReasoningEffort?: string
}

export type ModelOverrides = {
  mentorModel?: string
  executorModel?: string
}

export function resolveEffectiveModels(
  pair: PairLike,
  overrides?: ModelOverrides
): { mentorModel: string; executorModel: string } {
  // A legacy qualified id (`claude:opus`, `codex/mini`) is normalised here, once,
  // so every consumer — the pair's own mirror, the update payload and the
  // backend — sees the same bare model id.
  return {
    mentorModel: stripModelQualifier(
      overrides?.mentorModel ?? pair.pendingMentorModel ?? pair.mentorModel
    ),
    executorModel: stripModelQualifier(
      overrides?.executorModel ?? pair.pendingExecutorModel ?? pair.executorModel
    )
  }
}

/**
 * Resolve the models to assign for a task. Identical resolution order to
 * {@link resolveEffectiveModels} — kept as a named alias for the task-assignment
 * call site (`AssignTaskModal`) where "restoring models" reads more clearly.
 */
export function getAssignableTaskModels(
  pair: PairLike,
  restoringModels?: ModelOverrides
): { mentorModel: string; executorModel: string } {
  return resolveEffectiveModels(pair, restoringModels)
}

export function buildUpdateModelsPayload(
  pair: PairLike,
  effectiveModels: { mentorModel: string; executorModel: string }
): PairModelSelection {
  // `pair_update_models` overwrites the reasoning efforts with whatever the
  // payload carries, so the current ones must be sent back or they are cleared.
  // A role whose model changes gets the default effort instead: efforts are
  // model-specific (Claude `max` is not a Codex level), and the task modals
  // have no effort picker to fix a mismatch.
  const current = resolveEffectiveModels(pair)
  return {
    mentorModel: pair.mentorModel,
    executorModel: pair.executorModel,
    pendingMentorModel: stripModelQualifier(effectiveModels.mentorModel),
    pendingExecutorModel: stripModelQualifier(effectiveModels.executorModel),
    mentorReasoningEffort: modelIdsEquivalent(current.mentorModel, effectiveModels.mentorModel)
      ? pair.mentorReasoningEffort
      : undefined,
    executorReasoningEffort: modelIdsEquivalent(
      current.executorModel,
      effectiveModels.executorModel
    )
      ? pair.executorReasoningEffort
      : undefined
  }
}

/**
 * Determines whether updateModels should be called before assignTask.
 * Returns true only when explicit overrides are provided.
 *
 * @param overrides - The model overrides passed to assignTask
 * @returns true if updateModels should be called, false otherwise
 *
 * Design rationale:
 * - When overrides is undefined, backend already has correct models (pending or default)
 * - Calling updateModels only with explicit overrides avoids unnecessary IPC
 * - This prevents partial backend state on assignTask failure in the common case
 */
export function shouldSyncModelsToBackend(overrides: ModelOverrides | undefined): boolean {
  return overrides !== undefined
}
