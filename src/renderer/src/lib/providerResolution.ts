import type { AvailableModel, CreatePairInput, ProviderKind } from '../types'

/** Providers that can appear as a `<provider>/` qualifier on a stored model id. */
const QUALIFIER_PROVIDERS: ReadonlySet<ProviderKind> = new Set<ProviderKind>([
  'codex',
  'claude',
  'gemini',
  'kimi',
  'pi',
  'kiro',
  'aider',
  'grok',
  'muse'
])

/**
 * Providers whose stored model id keeps its `<provider>/` qualifier. Their bare ids
 * are not reliably re-inferable: Kimi/Pi/Kiro/Aider aliases are arbitrary names or
 * multi-provider paths ("ark-plan/glm-5", "anthropic/claude-sonnet-4"), Grok aliases
 * (`[model.<alias>]`), the Muse settings model and Codex `codex-*` slugs carry no
 * provider keyword. The backend strips the qualifier at spawn time.
 */
const QUALIFIED_STORAGE_PROVIDERS: ReadonlySet<ProviderKind> = new Set<ProviderKind>([
  'codex',
  'kimi',
  'pi',
  'kiro',
  'aider',
  'grok',
  'muse'
])

/**
 * Providers whose ids older builds stored bare (without the qualifier). Lookups
 * still accept those legacy ids so existing pairs and snapshots keep resolving.
 */
const LEGACY_BARE_PROVIDERS: ReadonlySet<ProviderKind> = new Set<ProviderKind>([
  'claude',
  'codex',
  'gemini',
  'grok',
  'muse'
])

function asQualifierProvider(prefix: string): ProviderKind | undefined {
  const lower = prefix.toLowerCase() as ProviderKind
  return QUALIFIER_PROVIDERS.has(lower) ? lower : undefined
}

/** Split `<provider>/<model>` into its parts; ids without a known provider qualifier stay whole. */
function splitQualifier(modelId: string): { provider?: ProviderKind; bare: string } {
  const slash = modelId.indexOf('/')
  if (slash <= 0 || slash === modelId.length - 1) return { bare: modelId }
  const provider = asQualifierProvider(modelId.slice(0, slash))
  return provider ? { provider, bare: modelId.slice(slash + 1) } : { bare: modelId }
}

/** The id a catalog entry is selected by in the UI (`provider/modelId`, raw id for OpenCode). */
function qualifiedIdOf(model: AvailableModel): string {
  return model.provider === 'opencode' ? model.modelId : `${model.provider}/${model.modelId}`
}

/**
 * True when two stored model ids name the same model: identical, or one is the
 * provider-qualified form of the other's legacy bare id (`codex/gpt-5` ≡ `gpt-5`).
 */
export function modelIdsEquivalent(a: string, b: string): boolean {
  if (a === b) return true
  const left = splitQualifier(a)
  const right = splitQualifier(b)
  if (left.provider && !right.provider) {
    return LEGACY_BARE_PROVIDERS.has(left.provider) && left.bare === b
  }
  if (right.provider && !left.provider) {
    return LEGACY_BARE_PROVIDERS.has(right.provider) && right.bare === a
  }
  return false
}

/** True when `storedId` (qualified or legacy bare) refers to this catalog entry. */
export function modelMatchesId(model: AvailableModel, storedId: string): boolean {
  if (qualifiedIdOf(model) === storedId) return true
  return LEGACY_BARE_PROVIDERS.has(model.provider) && model.modelId === storedId
}

/**
 * Find the catalog entry for a stored model id. Accepts the provider-qualified id
 * the UI stores (`codex/gpt-5`) as well as legacy bare ids (`gpt-5`); exact
 * qualified matches win, and an ambiguous bare id prefers the provider it infers to.
 */
export function getModelByQualifiedId(
  models: AvailableModel[],
  qualifiedModelId: string
): AvailableModel | undefined {
  const exact = models.find((model) => qualifiedIdOf(model) === qualifiedModelId)
  if (exact) return exact

  const legacy = models.filter((model) => modelMatchesId(model, qualifiedModelId))
  if (legacy.length <= 1) return legacy[0]
  const inferred = inferProviderFromModel(qualifiedModelId)
  return legacy.find((model) => model.provider === inferred) ?? legacy[0]
}

/**
 * Guess the provider from a model id. Kept in lockstep with Rust
 * `ProviderAdapter::infer_provider_kind` (all checks case-insensitive): a known
 * `<provider>/` qualifier wins, other slashed or `opencode*` ids are OpenCode,
 * and bare ids route by keyword, then `gpt` / `o<digit>` / a `codex-` prefix to Codex.
 */
export function inferProviderFromModel(modelId: string): ProviderKind {
  const lower = modelId.toLowerCase()
  if (lower.startsWith('opencode') || lower.includes('/')) {
    const [prefix, ...rest] = lower.split('/')
    if (rest.length > 0) {
      return asQualifierProvider(prefix) ?? 'opencode'
    }
    return 'opencode'
  }

  if (lower.includes('claude')) return 'claude'
  if (lower.includes('gemini')) return 'gemini'
  if (lower.includes('kimi')) return 'kimi'
  if (lower.includes('grok')) return 'grok'
  if (lower.includes('aider')) return 'aider'
  if (lower.includes('muse')) return 'muse'
  if (lower.includes('gpt') || /^o\d/.test(lower) || lower.startsWith('codex-')) {
    return 'codex'
  }

  return 'opencode'
}

/**
 * Provider for a (possibly changed) model id. An unchanged model keeps its known
 * provider — re-guessing from the id can misroute aliases — otherwise the catalog
 * entry decides, falling back to inference when the catalog does not list it.
 */
export function resolveModelProvider(
  models: AvailableModel[],
  modelId: string,
  previous?: { modelId: string; provider: ProviderKind }
): ProviderKind {
  if (previous && modelIdsEquivalent(previous.modelId, modelId)) {
    return previous.provider
  }
  return getModelByQualifiedId(models, modelId)?.provider ?? inferProviderFromModel(modelId)
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

  // Codex/Kimi/Pi/Kiro/Aider/Grok/Muse ids keep their qualifier (see
  // QUALIFIED_STORAGE_PROVIDERS): provider re-inference on model updates and
  // snapshot recovery depends on it. Claude and Gemini ids self-identify and
  // OpenCode ids are already provider-qualified by nature.
  return {
    role,
    provider: selected.provider,
    model: QUALIFIED_STORAGE_PROVIDERS.has(selected.provider)
      ? `${selected.provider}/${selected.modelId}`
      : selected.modelId
  }
}
