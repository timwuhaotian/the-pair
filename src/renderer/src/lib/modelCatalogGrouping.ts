import type { ModelRoute, ProviderView } from '../types'
import { modelIdsEquivalent } from './providerResolution'

export type AgentRole = 'mentor' | 'executor'

/** One reasoning effort a model accepts. */
export interface EffortOption {
  value: string
}

/** One endpoint's way to reach a model. */
export interface EndpointRoute {
  /** Stable identity for last-used-route persistence. */
  key: string
  providerId: string
  label: string
  vendor: string
  baseUrl: string
  /** True when the endpoint has at least one ready account. */
  ready: boolean
  /** The id this endpoint lists the model under. */
  modelId: string
  /** Reasoning efforts the endpoint accepts for this model. */
  efforts: string[]
  defaultEffort?: string
}

/** A model as the user thinks of it, merged across every endpoint that lists it. */
export interface CanonicalModel {
  canonicalKey: string
  displayName: string
  /** True when at least one endpoint can run it right now. */
  ready: boolean
  routes: EndpointRoute[]
}

export interface ResolvedSelection {
  model?: CanonicalModel
  route?: EndpointRoute
  effort?: EffortOption
}

/** One registry endpoint and the models it lists — step one of the picker. */
export interface EndpointOption {
  key: string
  label: string
  vendor: string
  baseUrl: string
  /** True when the endpoint has at least one ready account. */
  ready: boolean
  models: ModelRoute[]
}

/** The endpoints that share a vendor, ready ones first. */
export interface EndpointGroup {
  vendor: string
  label: string
  endpoints: EndpointOption[]
}

const LAST_ROUTE_KEY_PREFIX = 'the-pair-last-route-'

/** A model's identity across endpoints: the vendor that owns it plus its id.
 *  Lower-cased, the way the Rust resolver matches ids: two endpoints of one
 *  vendor listing `MiniMax-M3` and `minimax-m3` are the same model, not two rows. */
function canonicalKeyOf(provider: ProviderView, model: ModelRoute): string {
  return `${provider.vendor.toLowerCase()}::${model.id.toLowerCase()}`
}

function isReady(provider: ProviderView): boolean {
  return provider.readyAccountCount > 0
}

function byReadiness(a: ProviderView, b: ProviderView): number {
  return Number(isReady(b)) - Number(isReady(a))
}

function buildRoute(provider: ProviderView, model: ModelRoute): EndpointRoute {
  return {
    key: `${provider.id}::${model.id}`,
    providerId: provider.id,
    label: provider.displayName,
    vendor: provider.vendor,
    baseUrl: provider.baseUrl,
    ready: isReady(provider),
    modelId: model.id,
    efforts: model.efforts,
    defaultEffort: model.defaultEffort
  }
}

/** Collapse the registry into canonical models, each with one route per endpoint. */
export function buildCanonicalModels(providers: ProviderView[]): CanonicalModel[] {
  const groups = new Map<string, { rows: Array<{ provider: ProviderView; model: ModelRoute }> }>()
  for (const provider of providers) {
    for (const model of provider.models) {
      const key = canonicalKeyOf(provider, model)
      const existing = groups.get(key)
      if (existing) existing.rows.push({ provider, model })
      else groups.set(key, { rows: [{ provider, model }] })
    }
  }

  const result: CanonicalModel[] = []
  for (const [canonicalKey, { rows }] of groups) {
    const routes = rows.map(({ provider, model }) => buildRoute(provider, model))
    // Ready endpoints first, then by label, so the default route is the usable one.
    routes.sort((a, b) => {
      if (a.ready !== b.ready) return a.ready ? -1 : 1
      return a.label.localeCompare(b.label)
    })

    // Name the model after a route that can actually run it.
    const preferred = rows.find(({ provider }) => isReady(provider)) ?? rows[0]
    result.push({
      canonicalKey,
      displayName: preferred.model.displayName,
      ready: routes.some((route) => route.ready),
      routes
    })
  }

  result.sort((a, b) => {
    if (a.ready !== b.ready) return a.ready ? -1 : 1
    return a.displayName.localeCompare(b.displayName)
  })
  return result
}

/** The default effort for a route: the model's own default, else `medium` when offered. */
export function defaultLeafForRoute(route: EndpointRoute): {
  modelId: string
  reasoningEffort?: string
} {
  const effort = route.defaultEffort ?? (route.efforts.includes('medium') ? 'medium' : undefined)
  return { modelId: route.modelId, reasoningEffort: effort }
}

/** The Rust resolver lower-cases both ids before comparing, so case alone never separates two ids. */
function sameModelId(modelId: string, value: string): boolean {
  return modelId.toLowerCase() === value.toLowerCase()
}

function findRoute(
  models: CanonicalModel[],
  matches: (modelId: string) => boolean,
  reasoningEffort?: string
): ResolvedSelection | undefined {
  for (const model of models) {
    for (const route of model.routes) {
      if (!matches(route.modelId)) continue
      // An effort the endpoint does not list is not a selection — the caller gets
      // the route back and can fall back to the model's default.
      const effort =
        reasoningEffort !== undefined && route.efforts.includes(reasoningEffort)
          ? { value: reasoningEffort }
          : undefined
      return { model, route, effort }
    }
  }
  return undefined
}

/**
 * Find which model / route / effort a stored (model, effort) pair points at.
 * `value` is normally a plain model id; the qualified spelling older builds wrote
 * (`codex/gpt-5`) is matched when no exact id does.
 */
export function resolveSelection(
  models: CanonicalModel[],
  value: string,
  reasoningEffort?: string
): ResolvedSelection {
  return (
    findRoute(models, (modelId) => sameModelId(modelId, value), reasoningEffort) ??
    findRoute(models, (modelId) => modelIdsEquivalent(modelId, value), reasoningEffort) ??
    {}
  )
}

function getLastRouteKey(role: AgentRole, canonicalKey: string): string | undefined {
  try {
    const raw = localStorage.getItem(LAST_ROUTE_KEY_PREFIX + role)
    if (!raw) return undefined
    const parsed = JSON.parse(raw) as Record<string, string>
    return parsed[canonicalKey]
  } catch {
    return undefined
  }
}

export function saveLastRouteKey(role: AgentRole, canonicalKey: string, routeKey: string): void {
  try {
    const raw = localStorage.getItem(LAST_ROUTE_KEY_PREFIX + role)
    const parsed = raw ? (JSON.parse(raw) as Record<string, string>) : {}
    parsed[canonicalKey] = routeKey
    localStorage.setItem(LAST_ROUTE_KEY_PREFIX + role, JSON.stringify(parsed))
  } catch {
    // Ignore storage failures in restricted environments.
  }
}

/** The route to pre-select for a model: last-used for this role, else the first usable one. */
export function pickDefaultRoute(
  model: CanonicalModel,
  role: AgentRole
): EndpointRoute | undefined {
  const lastKey = getLastRouteKey(role, model.canonicalKey)
  if (lastKey) {
    const remembered = model.routes.find((route) => route.key === lastKey && route.ready)
    if (remembered) return remembered
  }
  // routes are already sorted ready-first. Nothing ready means nothing usable:
  // handing back an unauthenticated route would apply a model the pair cannot run.
  return model.routes.find((route) => route.ready)
}

function fuzzyMatch(text: string, normalizedQuery: string): boolean {
  const normalized = text.toLowerCase().replace(/[\s._-]/g, '')
  let qi = 0
  for (let i = 0; i < normalized.length && qi < normalizedQuery.length; i++) {
    if (normalized[i] === normalizedQuery[qi]) qi++
  }
  return qi === normalizedQuery.length
}

/** Subsequence-fuzzy match a query against a model's name and its endpoints' labels/ids. */
export function modelMatchesQuery(model: CanonicalModel, query: string): boolean {
  const normalized = query.toLowerCase().replace(/[\s._-]/g, '')
  if (normalized.length === 0) return true
  if (fuzzyMatch(model.displayName, normalized)) return true
  return model.routes.some(
    (route) =>
      fuzzyMatch(route.label, normalized) ||
      fuzzyMatch(route.vendor, normalized) ||
      fuzzyMatch(route.modelId, normalized)
  )
}

/** The registry endpoints, grouped by vendor, ready ones first. */
export function groupEndpoints(providers: ProviderView[]): EndpointGroup[] {
  const groups = new Map<string, EndpointGroup>()
  for (const provider of [...providers].sort(byReadiness)) {
    const option: EndpointOption = {
      key: provider.id,
      label: provider.displayName,
      vendor: provider.vendor,
      baseUrl: provider.baseUrl,
      ready: isReady(provider),
      models: provider.models
    }
    const existing = groups.get(provider.vendor)
    if (existing) existing.endpoints.push(option)
    else
      groups.set(provider.vendor, {
        vendor: provider.vendor,
        label: option.label,
        endpoints: [option]
      })
  }

  return Array.from(groups.values()).sort((a, b) => {
    const readyA = a.endpoints.some((endpoint) => endpoint.ready)
    const readyB = b.endpoints.some((endpoint) => endpoint.ready)
    if (readyA !== readyB) return readyA ? -1 : 1
    return a.label.localeCompare(b.label)
  })
}

/** The vendor brand behind a canonical model ("anthropic" for "anthropic::…"). */
export function modelVendorKey(canonicalKey: string): string {
  return canonicalKey.split('::')[0] ?? ''
}
