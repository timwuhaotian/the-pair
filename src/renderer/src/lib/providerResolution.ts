import type { AvailableModel, CreatePairInput, ProviderKind } from '../types'

export function getModelByQualifiedId(
  models: AvailableModel[],
  qualifiedModelId: string
): AvailableModel | undefined {
  return models.find((model) => {
    if (model.provider === 'opencode') {
      return model.modelId === qualifiedModelId
    }

    return `${model.provider}/${model.modelId}` === qualifiedModelId
  })
}

export function inferProviderFromModel(modelId: string): ProviderKind {
  if (modelId.startsWith('opencode') || modelId.includes('/')) {
    const prefix = modelId.split('/')[0]
    if (prefix === 'codex' || prefix === 'claude' || prefix === 'gemini') {
      return prefix
    }
    return 'opencode'
  }

  if (modelId.includes('claude')) return 'claude'
  if (modelId.includes('gemini')) return 'gemini'
  if (modelId.includes('gpt') || /^o\d/.test(modelId)) {
    return 'codex'
  }

  return 'opencode'
}

export function buildAgentConfig(
  role: 'mentor' | 'executor',
  modelId: string,
  models: AvailableModel[]
): CreatePairInput['mentor'] {
  const selected = getModelByQualifiedId(models, modelId)
  if (!selected) {
    throw new Error(`Selected ${role} model is not available: ${modelId}`)
  }

  return {
    role,
    provider: selected.provider,
    model: selected.modelId
  }
}
