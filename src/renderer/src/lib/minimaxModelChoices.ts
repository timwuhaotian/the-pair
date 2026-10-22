import type { AvailableModel } from '../types'

/** Only classify known text IDs; custom aliases and media models keep their place. */
function textModelId(modelId: string): string | undefined {
  const parts = modelId.toLowerCase().split('/')
  if (parts[0] === 'pi') parts.shift()
  if (
    parts.length > 1 &&
    ![
      'minimax',
      'minimax-cn',
      'minimax-coding-plan',
      'minimax-cn-coding-plan',
      'opencode',
      'openrouter'
    ].includes(parts[0])
  )
    return undefined
  const id = parts.pop()
  return id && /^minimax-(?:m\d+(?:\.\d+)?(?:-[a-z0-9]+)*|text-01)(?::[a-z]+)?$/.test(id)
    ? id
    : undefined
}

export function isOrdinaryModelChoice(model: AvailableModel): boolean {
  // The registry marks explicitly configured OpenCode models before merging discovery.
  if (model.planLabel === 'custom-provider' || !['opencode', 'pi'].includes(model.provider))
    return true
  const id = textModelId(model.modelId)
  return (
    id === undefined ||
    /^minimax-m3(?::[a-z]+)?$/.test(id) ||
    /^minimax-m3\.1-flash(?:-free|:free)?$/.test(id)
  )
}

/** Reorder only MiniMax text slots so unrelated model preferences stay unchanged. */
export function preferMiniMaxFlash<T>(models: T[], modelId: (model: T) => string): T[] {
  const isText = (model: T): boolean => textModelId(modelId(model)) !== undefined
  const textModels = models.filter(isText).sort((a, b) => {
    const rank = (model: T): number => {
      const id = textModelId(modelId(model)) ?? ''
      if (/^minimax-m3\.1-flash(?:-free|:free)?$/.test(id)) return 0
      return /^minimax-m3(?::[a-z]+)?$/.test(id) ? 1 : 2
    }
    return rank(a) - rank(b)
  })
  let index = 0
  return models.map((model) => (isText(model) ? textModels[index++] : model))
}

export function ordinaryModelChoices(models: AvailableModel[]): AvailableModel[] {
  return preferMiniMaxFlash(models, (model) =>
    model.planLabel === 'custom-provider' || !['opencode', 'pi'].includes(model.provider)
      ? ''
      : model.modelId
  ).filter(isOrdinaryModelChoice)
}
