import type { AvailableModel } from '../types'
import { getQualifiedModel, isSelectableForPairExecution } from './modelPreferences'

export type AgentRole = 'mentor' | 'executor'

/**
 * A single selectable "leaf" of a route — one reasoning effort. For providers that bake
 * effort into the model id (Antigravity) each effort has a distinct `qualifiedId`; for
 * providers that take effort as a runtime flag (Codex) the `qualifiedId` is shared and
 * `reasoningEffort` carries the value.
 */
export interface EffortOption {
  /** Canonical effort id: 'low' | 'medium' | 'high'. */
  value: string
  /** Qualified model id to set as the picker value when this effort is chosen. */
  qualifiedId: string
  /** reasoning_effort flag to pass through; undefined when effort is baked into the id. */
  reasoningEffort?: string
}

/** One way to reach a canonical model: a provider + plan, with its effort axis (if any). */
export interface ModelRoute {
  /** Stable identity for last-used-route persistence. */
  key: string
  provider: AvailableModel['provider']
  providerLabel: string
  /** Minimal plan/access hint ("Claude Code login", "OpenAI API key"). */
  accessLabel: string
  planLabel?: string
  billingKind: AvailableModel['billingKind']
  available: boolean
  /** Representative qualified id used when the route exposes no effort axis. */
  baseQualifiedId: string
  /** Effort leaves; empty when the route exposes no effort axis. */
  effortOptions: EffortOption[]
}

/** A model as the user thinks of it, merged across every route that can run it. */
export interface CanonicalModel {
  canonicalKey: string
  displayName: string
  available: boolean
  routes: ModelRoute[]
}

export interface ResolvedSelection {
  model?: CanonicalModel
  route?: ModelRoute
  effort?: EffortOption
}

const EFFORT_ORDER: Record<string, number> = { low: 0, medium: 1, high: 2 }
// Preference order when there is no remembered route: plan-included native routes before
// pay-as-you-go OpenCode, so the user does not accidentally spend on an API key.
const PROVIDER_PRIORITY: Record<string, number> = { claude: 0, codex: 1, gemini: 2, opencode: 3 }
const LAST_ROUTE_KEY_PREFIX = 'the-pair-last-route-'

/**
 * Canonical merge identity. Trusts the backend-provided key; falls back to a
 * provider+bare-id heuristic for resilience (older cached data or test fixtures).
 */
function canonicalKeyOf(model: AvailableModel): string {
  if (model.canonicalKey && model.canonicalKey.length > 0) {
    return model.canonicalKey
  }
  const base = (model.modelId.split('/').pop() ?? model.modelId).toLowerCase()
  return `${model.provider}::${base}`
}

function routeKeyOf(model: AvailableModel): string {
  return `${model.provider}::${model.planLabel ?? model.providerLabel}`
}

function displayNameOf(model: AvailableModel): string {
  const canonical = model.canonicalDisplayName?.trim()
  return canonical && canonical.length > 0 ? canonical : model.displayName
}

function buildRoute(key: string, rows: AvailableModel[]): ModelRoute {
  const first = rows[0]
  const effortOptions: EffortOption[] = []

  const baked = rows.filter((row) => row.effortTag)
  if (baked.length > 0) {
    for (const row of baked) {
      effortOptions.push({
        value: row.effortTag as string,
        qualifiedId: getQualifiedModel(row),
        reasoningEffort: undefined
      })
    }
    effortOptions.sort((a, b) => (EFFORT_ORDER[a.value] ?? 99) - (EFFORT_ORDER[b.value] ?? 99))
  } else if (first.reasoningEffortLevels && first.reasoningEffortLevels.length > 0) {
    for (const level of first.reasoningEffortLevels) {
      effortOptions.push({
        value: level,
        qualifiedId: getQualifiedModel(first),
        reasoningEffort: level
      })
    }
  }

  const readyRow = rows.find((row) => isSelectableForPairExecution(row)) ?? first
  const baseQualifiedId =
    effortOptions.length > 0
      ? (effortOptions.find((option) => option.value === 'medium') ?? effortOptions[0]).qualifiedId
      : getQualifiedModel(readyRow)

  return {
    key,
    provider: first.provider,
    providerLabel: first.providerLabel,
    accessLabel: first.accessLabel,
    planLabel: first.planLabel,
    billingKind: first.billingKind,
    available: rows.some((row) => isSelectableForPairExecution(row)),
    baseQualifiedId,
    effortOptions
  }
}

/** Collapse a flat model list into canonical models, each with its routes and effort leaves. */
export function buildCanonicalModels(models: AvailableModel[]): CanonicalModel[] {
  const groups = new Map<string, AvailableModel[]>()
  for (const model of models) {
    const key = canonicalKeyOf(model)
    const existing = groups.get(key)
    if (existing) existing.push(model)
    else groups.set(key, [model])
  }

  const result: CanonicalModel[] = []
  for (const [canonicalKey, rows] of groups) {
    const routeMap = new Map<string, AvailableModel[]>()
    for (const row of rows) {
      const rk = routeKeyOf(row)
      const existing = routeMap.get(rk)
      if (existing) existing.push(row)
      else routeMap.set(rk, [row])
    }

    const routes = Array.from(routeMap.entries()).map(([rk, routeRows]) =>
      buildRoute(rk, routeRows)
    )
    routes.sort((a, b) => {
      if (a.available !== b.available) return a.available ? -1 : 1
      const pa = PROVIDER_PRIORITY[a.provider] ?? 9
      const pb = PROVIDER_PRIORITY[b.provider] ?? 9
      if (pa !== pb) return pa - pb
      return a.providerLabel.localeCompare(b.providerLabel)
    })

    const preferredRow =
      rows.find((row) => row.provider !== 'opencode' && isSelectableForPairExecution(row)) ??
      rows.find((row) => isSelectableForPairExecution(row)) ??
      rows[0]

    result.push({
      canonicalKey,
      displayName: displayNameOf(preferredRow),
      available: routes.some((route) => route.available),
      routes
    })
  }

  result.sort((a, b) => {
    if (a.available !== b.available) return a.available ? -1 : 1
    return a.displayName.localeCompare(b.displayName)
  })
  return result
}

/** The selection a route resolves to by default (medium effort when present). */
export function defaultLeafForRoute(route: ModelRoute): {
  qualifiedId: string
  reasoningEffort?: string
} {
  if (route.effortOptions.length > 0) {
    const chosen =
      route.effortOptions.find((option) => option.value === 'medium') ?? route.effortOptions[0]
    return { qualifiedId: chosen.qualifiedId, reasoningEffort: chosen.reasoningEffort }
  }
  return { qualifiedId: route.baseQualifiedId, reasoningEffort: undefined }
}

/** Find which model / route / effort a stored (value, reasoningEffort) pair points at. */
export function resolveSelection(
  models: CanonicalModel[],
  value: string,
  reasoningEffort?: string
): ResolvedSelection {
  for (const model of models) {
    for (const route of model.routes) {
      if (route.effortOptions.length > 0) {
        const exact = route.effortOptions.find(
          (option) =>
            option.qualifiedId === value &&
            (option.reasoningEffort === undefined || option.reasoningEffort === reasoningEffort)
        )
        if (exact) return { model, route, effort: exact }
        if (route.effortOptions.some((option) => option.qualifiedId === value)) {
          // Same model id, effort flag not matched yet (e.g. an unset Codex effort).
          return { model, route, effort: undefined }
        }
      } else if (route.baseQualifiedId === value) {
        return { model, route, effort: undefined }
      }
    }
  }
  return {}
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

/** The route to pre-select for a model: last-used for this role, else preference order. */
export function pickDefaultRoute(model: CanonicalModel, role: AgentRole): ModelRoute | undefined {
  if (model.routes.length === 0) return undefined
  const lastKey = getLastRouteKey(role, model.canonicalKey)
  if (lastKey) {
    const remembered = model.routes.find((route) => route.key === lastKey && route.available)
    if (remembered) return remembered
  }
  // routes are already sorted available-first, then by provider priority.
  return model.routes.find((route) => route.available) ?? model.routes[0]
}

function fuzzyMatch(text: string, normalizedQuery: string): boolean {
  const normalized = text.toLowerCase().replace(/[\s._-]/g, '')
  let qi = 0
  for (let i = 0; i < normalized.length && qi < normalizedQuery.length; i++) {
    if (normalized[i] === normalizedQuery[qi]) qi++
  }
  return qi === normalizedQuery.length
}

/** Subsequence-fuzzy match a query against a model's name and its routes' labels/ids. */
export function modelMatchesQuery(model: CanonicalModel, query: string): boolean {
  const normalized = query.toLowerCase().replace(/[\s._-]/g, '')
  if (normalized.length === 0) return true
  if (fuzzyMatch(model.displayName, normalized)) return true
  return model.routes.some(
    (route) =>
      fuzzyMatch(route.providerLabel, normalized) ||
      fuzzyMatch(route.accessLabel, normalized) ||
      fuzzyMatch(route.baseQualifiedId, normalized)
  )
}
