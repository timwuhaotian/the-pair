import type { ModelRoute, ProviderView } from '../types'
import { modelMatchesId } from './providerResolution'
import { ordinaryModelChoices } from './minimaxModelChoices'

const PREFERRED_MODEL_KEYS = {
  mentor: 'the-pair-preferred-mentor-model',
  executor: 'the-pair-preferred-executor-model'
} as const

/** An endpoint a pair can run models on: at least one ready account, and something to run. */
export function isSelectableForPairExecution(provider: ProviderView): boolean {
  return provider.readyAccountCount > 0 && provider.models.length > 0
}

function selectableModels(providers: ProviderView[]): ModelRoute[] {
  return providers.filter(isSelectableForPairExecution).flatMap((provider) => provider.models)
}

export function savePreferredModelId(role: 'mentor' | 'executor', modelId: string): void {
  try {
    localStorage.setItem(PREFERRED_MODEL_KEYS[role], modelId)
  } catch {
    // Ignore storage failures in restricted environments.
  }
}

export function getPreferredModelId(role: 'mentor' | 'executor'): string {
  try {
    return localStorage.getItem(PREFERRED_MODEL_KEYS[role]) || ''
  } catch {
    return ''
  }
}

/**
 * The model id to pre-select for a role: the remembered choice when the registry
 * still offers it, else the first default choice. Keys are unchanged from the
 * agent-CLI builds, so a stored qualified id (`claude/sonnet`) still resolves and
 * is rewritten to its plain spelling on the way through.
 */
export function resolvePreferredModel(
  role: 'mentor' | 'executor',
  providers: ProviderView[]
): string {
  const models = selectableModels(providers)
  const preferred = getPreferredModelId(role)
  if (preferred) {
    const match = models.find((model) => modelMatchesId(model, preferred))
    if (match) {
      if (match.id !== preferred) savePreferredModelId(role, match.id)
      return match.id
    }
  }

  const [fallback] = ordinaryModelChoices(models)
  return fallback ? fallback.id : ''
}
