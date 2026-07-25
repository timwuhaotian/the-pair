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

export function resolveEffectiveModels(
  pair: PairLike,
  overrides?: ModelOverrides
): { mentorModel: string; executorModel: string } {
  return {
    mentorModel: overrides?.mentorModel ?? pair.pendingMentorModel ?? pair.mentorModel,
    executorModel: overrides?.executorModel ?? pair.pendingExecutorModel ?? pair.executorModel
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

/**
 * Strip the provider prefix from a qualified model ID.
 *
 * The frontend uses "qualified" IDs like `claude/claude-haiku-4-5-20251001`
 * for model selection and localStorage. The backend and CLI tools expect bare
 * IDs (e.g. `claude-haiku-4-5-20251001`). OpenCode IDs already use
 * `provider/model` format internally and must not be stripped.
 */
function stripProviderPrefix(qualifiedId: string): string {
  if (qualifiedId.includes('/')) {
    const [prefix, ...rest] = qualifiedId.split('/')
    // `kimi` is deliberately absent: Kimi aliases are arbitrary user-defined
    // names, so the `kimi/` qualifier must survive in stored ids for provider
    // re-inference. The Rust provider strips it at spawn time instead.
    if (['claude', 'codex', 'gemini'].includes(prefix) && rest.length > 0) {
      return rest.join('/')
    }
  }
  return qualifiedId
}

export function buildUpdateModelsPayload(
  pair: PairLike,
  effectiveModels: { mentorModel: string; executorModel: string }
): PairModelSelection {
  return {
    mentorModel: pair.mentorModel,
    executorModel: pair.executorModel,
    pendingMentorModel: stripProviderPrefix(effectiveModels.mentorModel),
    pendingExecutorModel: stripProviderPrefix(effectiveModels.executorModel)
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
