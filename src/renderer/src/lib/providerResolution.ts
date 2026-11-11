import type { ModelRoute, ProviderView } from '../types'

/**
 * Qualifier tokens older builds put in front of a model id (`claude/sonnet`,
 * `codex:gpt-5`). A pair used to store one id per agent CLI, so those prefixes
 * have to be recognised to read legacy snapshots and localStorage back.
 *
 * Gateway path segments (`anthropic/`, `openai/`, `openrouter/`) are deliberately
 * absent: they are how a gateway names a model in its own catalog, and stripping
 * one would lose the route the id belongs to.
 */
const QUALIFIER_TOKENS: ReadonlySet<string> = new Set([
  'claude',
  'codex',
  'gemini',
  'kimi',
  'pi',
  'kiro',
  'aider',
  'grok',
  'muse',
  'opencode'
])

/** A bare model id: no path separators, no spaces — just a slug.
 *  Mirrors Rust `looks_like_id`: any of `[A-Za-z0-9._+-]` may come first. */
function looksLikeModelId(candidate: string): boolean {
  return candidate.length > 0 && /^[a-z0-9._+-]+$/i.test(candidate)
}

/**
 * Split `<qualifier>/<model>` (or `<qualifier>:<model>`) into its parts.
 * Ids whose remainder is not a plain slug stay whole. Both halves come back
 * lower-cased, the way the Rust resolver normalises an id before matching: it
 * lower-cases the whole string, so `claude:Opus-4-1` and `claude:opus-4-1`
 * name one model.
 */
function splitQualifier(modelId: string): { qualifier?: string; bare: string } {
  const trimmed = modelId.trim()
  const separator = trimmed.search(/[:/]/)
  if (separator <= 0 || separator === trimmed.length - 1) return { bare: trimmed.toLowerCase() }
  const qualifier = trimmed.slice(0, separator).toLowerCase()
  const bare = trimmed.slice(separator + 1)
  return QUALIFIER_TOKENS.has(qualifier) && looksLikeModelId(bare)
    ? { qualifier, bare: bare.toLowerCase() }
    : { bare: trimmed.toLowerCase() }
}

/** Drop a legacy qualifier, leaving the plain, lower-cased model id the registry lists. */
export function stripModelQualifier(modelId: string): string {
  return splitQualifier(modelId).bare
}

/**
 * True when two stored model ids name the same model: identical, or one is the
 * qualified spelling of the other's legacy bare id (`codex/gpt-5` ≡ `gpt-5`).
 * Two differently-qualified ids are never equivalent — the qualifier is part of
 * what the id says. Comparison is case-insensitive, because the Rust resolver
 * lower-cases an id before it matches one.
 */
export function modelIdsEquivalent(a: string, b: string): boolean {
  const left = splitQualifier(a)
  const right = splitQualifier(b)
  if (left.qualifier && right.qualifier && left.qualifier !== right.qualifier) return false
  return left.bare === right.bare
}

/** True when `storedId` (qualified or legacy bare) refers to this registry model. */
export function modelMatchesId(model: ModelRoute, storedId: string): boolean {
  return model.id === storedId || modelIdsEquivalent(model.id, storedId)
}

/** Endpoints that can actually run a model come first, so a reachable route wins. */
function byReadiness(a: ProviderView, b: ProviderView): number {
  return Number(b.readyAccountCount > 0) - Number(a.readyAccountCount > 0)
}

function pickModel(
  providers: ProviderView[],
  predicate: (model: ModelRoute) => boolean
): ModelRoute | undefined {
  for (const provider of [...providers].sort(byReadiness)) {
    const model = provider.models.find(predicate)
    if (model) return model
  }
  return undefined
}

/**
 * Find the registry entry for a stored model id. Accepts the qualified spelling
 * older builds wrote (`codex/gpt-5`) as well as plain ids (`gpt-5`); exact
 * matches win, then legacy bare matches, and a ready endpoint breaks ties.
 */
export function getModelByQualifiedId(
  providers: ProviderView[],
  qualifiedModelId: string
): ModelRoute | undefined {
  return (
    pickModel(providers, (model) => model.id === qualifiedModelId) ??
    pickModel(providers, (model) => modelIdsEquivalent(model.id, qualifiedModelId))
  )
}
