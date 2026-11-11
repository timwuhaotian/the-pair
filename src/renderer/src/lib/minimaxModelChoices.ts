import type { ModelSource } from '../types'

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

/** The slice of a registry model row the MiniMax heuristics read. */
export interface ModelChoice {
  id: string
  source: ModelSource
}

/**
 * Rows imported from an agent config are the only ones whose MiniMax slots need
 * curating — that is where several generations of the same text model sit side by
 * side. Presets, endpoint discovery and hand-added providers are left untouched.
 */
function isImportedChoice(model: ModelChoice): boolean {
  return model.source === 'imported'
}

export function isOrdinaryModelChoice(model: ModelChoice): boolean {
  if (!isImportedChoice(model)) return true
  const id = textModelId(model.id)
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

export function ordinaryModelChoices<T extends ModelChoice>(models: T[]): T[] {
  const ordered = preferMiniMaxFlash(models, (model) => (isImportedChoice(model) ? model.id : ''))
  const hasCurrentTextChoice = models.some((model) => {
    if (!isImportedChoice(model)) return false
    const id = textModelId(model.id)
    return (
      id !== undefined &&
      (/^minimax-m3(?::[a-z]+)?$/.test(id) || /^minimax-m3\.1-flash(?:-free|:free)?$/.test(id))
    )
  })
  return hasCurrentTextChoice ? ordered.filter(isOrdinaryModelChoice) : ordered
}
