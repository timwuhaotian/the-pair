import type { PairModelSelection } from '../types'

export type PairLike = {
  mentorModel: string
  executorModel: string
  pendingMentorModel?: string
  pendingExecutorModel?: string
}

export type ModelOverrides = {
  mentorModel?: string
  executorModel?: string
}

export function getAssignableTaskModels(
  pair: PairLike,
  restoringModels?: ModelOverrides
): { mentorModel: string; executorModel: string } {
  return {
    mentorModel: restoringModels?.mentorModel ?? pair.pendingMentorModel ?? pair.mentorModel,
    executorModel: restoringModels?.executorModel ?? pair.pendingExecutorModel ?? pair.executorModel
  }
}

export function resolveEffectiveModels(
  pair: PairLike,
  overrides?: ModelOverrides
): { mentorModel: string; executorModel: string } {
  return {
    mentorModel: overrides?.mentorModel ?? pair.pendingMentorModel ?? pair.mentorModel,
    executorModel: overrides?.executorModel ?? pair.pendingExecutorModel ?? pair.executorModel
  }
}

export function buildUpdateModelsPayload(
  pair: PairLike,
  effectiveModels: { mentorModel: string; executorModel: string }
): PairModelSelection {
  return {
    mentorModel: pair.mentorModel,
    executorModel: pair.executorModel,
    pendingMentorModel: effectiveModels.mentorModel,
    pendingExecutorModel: effectiveModels.executorModel
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
