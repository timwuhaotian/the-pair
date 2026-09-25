import type { AvailableModel } from '../types'
import { modelMatchesId } from './providerResolution'

const PREFERRED_MODEL_KEYS = {
  mentor: 'the-pair-preferred-mentor-model',
  executor: 'the-pair-preferred-executor-model'
} as const

export function isSelectableForPairExecution(model: AvailableModel): boolean {
  return model.available && model.supportsPairExecution
}

export function getQualifiedModel(model: AvailableModel): string {
  if (model.provider === 'opencode') {
    return model.modelId
  }
  return `${model.provider}/${model.modelId}`
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

export function getPreferredQualifiedModel(
  role: 'mentor' | 'executor',
  models: AvailableModel[]
): string {
  const preferred = getPreferredModelId(role)
  if (preferred) {
    // Exact qualified match first; a legacy bare id resolves to its qualified form.
    const selectable = models.filter((model) => isSelectableForPairExecution(model))
    const match =
      selectable.find((model) => getQualifiedModel(model) === preferred) ??
      selectable.find((model) => modelMatchesId(model, preferred))
    if (match) return getQualifiedModel(match)
  }

  const defaultEntry = models.find((model) => isSelectableForPairExecution(model))
  return defaultEntry ? getQualifiedModel(defaultEntry) : ''
}

export function getPreferredPairModelSelection(models: AvailableModel[]): {
  mentorModel: string
  executorModel: string
} {
  return {
    mentorModel: getPreferredQualifiedModel('mentor', models),
    executorModel: getPreferredQualifiedModel('executor', models)
  }
}
