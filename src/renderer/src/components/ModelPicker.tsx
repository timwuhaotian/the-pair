import React, { useMemo, useState, useRef, useEffect, useCallback, useLayoutEffect } from 'react'
import { createPortal } from 'react-dom'
import { ChevronDown, ExternalLink, Search } from 'lucide-react'
import { emit } from '@tauri-apps/api/event'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import {
  Z,
  emptyHint,
  fieldLabel,
  groupHeader,
  hoverTint,
  menuItem,
  popoverSurface,
  searchInput,
  type Tone
} from '../lib/ui'
import { GlassButton } from './ui/GlassButton'
import { BrandIcon } from './ui/BrandIcon'
import { Chip } from './ui/Chip'
import { vendorIconKey } from '../lib/brandIcons'
import type { AccountStatus, ProviderView } from '../types'
import {
  getPreferredModelId,
  isSelectableForPairExecution,
  savePreferredModelId
} from '../lib/modelPreferences'
import {
  buildCanonicalModels,
  defaultLeafForRoute,
  groupEndpoints,
  modelMatchesQuery,
  modelVendorKey,
  pickDefaultRoute,
  resolveSelection,
  saveLastRouteKey,
  type CanonicalModel,
  type EffortOption,
  type EndpointGroup,
  type EndpointOption,
  type EndpointRoute
} from '../lib/modelCatalogGrouping'
import { ordinaryModelChoices, preferMiniMaxFlash } from '../lib/minimaxModelChoices'
import { isImeKeyEvent } from './keyboard'

const RECENT_MODELS_KEY_PREFIX = 'the-pair-recent-models-'
const MAX_RECENT_MODELS = 4

/** One endpoint as the picker lists it, with the registry row it came from. */
interface EndpointChoice {
  endpoint: EndpointOption
  provider: ProviderView
}

/** A remembered model, resolved against the whole registry. */
interface RecentChoice {
  /** The id as it was stored — may be a legacy qualified spelling. */
  storedId: string
  model: CanonicalModel
  route?: EndpointRoute
}

function effortTranslationKey(
  level: string
):
  | 'pickers.reasoning'
  | 'pickers.reasoningOff'
  | 'pickers.reasoningFast'
  | 'pickers.reasoningBalanced'
  | 'pickers.reasoningDeep'
  | undefined {
  switch (level) {
    case 'disabled':
      return 'pickers.reasoningOff'
    case 'low':
      return 'pickers.reasoningFast'
    case 'adaptive':
      return 'pickers.reasoning'
    case 'medium':
      return 'pickers.reasoningBalanced'
    case 'high':
      return 'pickers.reasoningDeep'
    default:
      return undefined
  }
}

// `buildCanonicalModels` walks the whole provider each call; the registry
// objects are stable across renders, so one entry per provider is enough.
const canonicalForCache = new WeakMap<ProviderView, CanonicalModel[]>()
function canonicalFor(provider: ProviderView): CanonicalModel[] {
  const hit = canonicalForCache.get(provider)
  if (hit) return hit
  const canonical = buildCanonicalModels([
    { ...provider, models: ordinaryModelChoices(provider.models) }
  ])
  canonicalForCache.set(provider, canonical)
  return canonical
}

/**
 * The listed choices for one endpoint: current MiniMax text slots only, Flash
 * first. `buildCanonicalModels` re-sorts by name, so the ordering is laid over
 * the result rather than before it.
 */
function choiceModelsFor(provider: ProviderView): CanonicalModel[] {
  return preferMiniMaxFlash(canonicalFor(provider), (model) => model.routes[0]?.modelId ?? '')
}

/** Endpoints grouped by vendor, each carrying the registry row it came from. */
function endpointGroupsFor(providers: ProviderView[]): Array<{
  group: EndpointGroup
  choices: EndpointChoice[]
}> {
  const byId = new Map(providers.map((provider) => [provider.id, provider]))
  return groupEndpoints(providers).flatMap((group) => {
    const choices = group.endpoints.flatMap((endpoint) => {
      const provider = byId.get(endpoint.key)
      return provider ? [{ endpoint, provider }] : []
    })
    return choices.length > 0 ? [{ group, choices }] : []
  })
}

/** Why an endpoint cannot run anything: the most actionable account state it has. */
function credentialBlocker(provider: ProviderView): AccountStatus {
  const order: AccountStatus[] = ['authExpired', 'rateLimited', 'needsKey']
  return (
    order.find((status) => provider.accounts.some((account) => account.status === status)) ??
    'needsKey'
  )
}

function getRecentModelIds(role: 'mentor' | 'executor'): string[] {
  try {
    const roleKey = RECENT_MODELS_KEY_PREFIX + role
    const stored = localStorage.getItem(roleKey)
    if (stored) return JSON.parse(stored)
    const legacy = localStorage.getItem('the-pair-recent-models')
    if (legacy) {
      const ids: string[] = JSON.parse(legacy)
      localStorage.setItem(roleKey, JSON.stringify(ids.slice(0, MAX_RECENT_MODELS)))
      return ids.slice(0, MAX_RECENT_MODELS)
    }
    return []
  } catch {
    return []
  }
}

function saveRecentModelId(role: 'mentor' | 'executor', modelId: string): string[] {
  const key = RECENT_MODELS_KEY_PREFIX + role
  const recent = getRecentModelIds(role).filter((id) => id !== modelId)
  recent.unshift(modelId)
  const next = recent.slice(0, MAX_RECENT_MODELS)
  localStorage.setItem(key, JSON.stringify(next))
  return next
}

export interface ModelPickerProps {
  value: string
  providers: ProviderView[]
  onChange: (value: string) => void
  role: 'mentor' | 'executor'
  variant?: 'card' | 'inline'
  dropUp?: boolean
  reasoningEffort?: string
  onReasoningEffortChange?: (value: string | undefined) => void
}

/**
 * Tell the backend which endpoint a model was picked under. Fire-and-forget:
 * a build with no Tauri host (plain browser, unit tests) has nothing to tell,
 * and a failure only means the id is resolved the way it was before the pick.
 */
function reportPickedRoute(role: 'mentor' | 'executor', modelId: string, providerId: string): void {
  try {
    const preferRoute = typeof window === 'undefined' ? undefined : window.api?.models?.preferRoute
    void preferRoute?.({ role, modelId, providerId })?.catch(() => undefined)
  } catch {
    // No IPC bridge to report to.
  }
}

function roleClasses(role: 'mentor' | 'executor'): { fg: string; border: string; bg: string } {
  if (role === 'mentor')
    return { fg: 'role-mentor', border: 'border-role-mentor', bg: 'bg-role-mentor' }
  return { fg: 'role-executor', border: 'border-role-executor', bg: 'bg-role-executor' }
}

function VendorIcon({ canonicalKey }: { canonicalKey: string }): React.ReactNode {
  const vendor = modelVendorKey(canonicalKey)
  return <BrandIcon icon={vendorIconKey(vendor)} label={vendor} className="self-center" />
}

/** Step one: one row per endpoint, whatever number of accounts sit behind it. */
function EndpointRow({
  choice,
  active,
  role,
  onSelect
}: {
  choice: EndpointChoice
  active: boolean
  role: 'mentor' | 'executor'
  onSelect: (endpoint: EndpointOption) => void
}): React.ReactNode {
  const { t } = useTranslation()
  const c = roleClasses(role)
  const { provider } = choice
  return (
    <button
      type="button"
      role="radio"
      aria-checked={active}
      aria-label={provider.displayName}
      title={`${provider.displayName} · ${provider.baseUrl}`}
      onClick={() => onSelect(choice.endpoint)}
      className={cn(
        'flex w-full cursor-pointer items-center gap-2 rounded-md border px-2 py-1 text-left transition-colors',
        active
          ? cn(c.border, c.bg)
          : cn('border-border opacity-80 hover:border-foreground/40 hover:opacity-100', hoverTint)
      )}
    >
      <span
        aria-hidden
        className={cn('select-none', active ? c.fg : 'text-muted-foreground-faint')}
      >
        {active ? '●' : '○'}
      </span>
      <BrandIcon
        icon={vendorIconKey(provider.vendor)}
        label={provider.vendor}
        className="size-3.5"
      />
      <span
        className="min-w-0 flex-1 truncate text-[12px] text-foreground/90 [overflow-wrap:anywhere]"
        title={provider.displayName}
      >
        {provider.displayName}
      </span>
      <span className="shrink-0 text-[10px] tabular-nums text-muted-foreground-faint">
        {t('models.accountCount', {
          ready: provider.readyAccountCount,
          total: provider.accounts.length
        })}
      </span>
    </button>
  )
}

const CREDENTIAL_TONES: Record<AccountStatus, Tone> = {
  ready: 'done',
  needsKey: 'running',
  authExpired: 'error',
  rateLimited: 'running'
}

/**
 * An endpoint no account can reach. It is not selectable — the credential is
 * added in the Models section — so the row itself is the way there.
 */
function BlockedEndpointRow({
  provider,
  onOpenModels
}: {
  provider: ProviderView
  onOpenModels: () => void
}): React.ReactNode {
  const { t } = useTranslation()
  const status = credentialBlocker(provider)
  return (
    <button
      type="button"
      onClick={onOpenModels}
      aria-label={`${provider.displayName} — ${t(`pickers.status.${status}`)}`}
      title={`${provider.displayName} · ${provider.baseUrl} · ${t('pickers.openModels')}`}
      className={cn(
        'flex w-full cursor-pointer items-center gap-2 rounded-md border border-border px-2 py-1 text-left transition-colors',
        hoverTint,
        'hover:border-foreground/40'
      )}
    >
      <BrandIcon
        icon={vendorIconKey(provider.vendor)}
        label={provider.vendor}
        className="size-3.5 opacity-50"
      />
      <span className="min-w-0 flex-1 truncate text-[12px] text-muted-foreground">
        {provider.displayName}
      </span>
      <Chip shape="tag" tone={CREDENTIAL_TONES[status]}>
        {t(`pickers.status.${status}`)}
      </Chip>
      <ExternalLink size={11} className="shrink-0 text-muted-foreground-faint" />
    </button>
  )
}

function QuickPickRow({
  choice,
  selected,
  role,
  onSelect
}: {
  choice: RecentChoice
  selected: boolean
  role: 'mentor' | 'executor'
  onSelect: (choice: RecentChoice) => void
}): React.ReactNode {
  const c = roleClasses(role)
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      title={choice.model.displayName}
      onClick={() => onSelect(choice)}
      className={cn(
        'flex w-full items-center gap-2 rounded-md border px-2 py-1 text-left transition-colors cursor-pointer text-[12px]',
        selected
          ? cn(c.border, c.bg, c.fg)
          : cn('border-border hover:border-foreground/40', hoverTint)
      )}
    >
      <span
        aria-hidden
        className={cn('select-none', selected ? c.fg : 'text-muted-foreground-faint')}
      >
        {selected ? '●' : '○'}
      </span>
      <VendorIcon canonicalKey={choice.model.canonicalKey} />
      {/* No title here: the button already carries the model name as its
          title, and a duplicate would break getByTitle queries. */}
      <span className="min-w-0 flex-1 truncate text-foreground/90 [overflow-wrap:anywhere]">
        {choice.model.displayName}
      </span>
      {choice.route && (
        <span
          title={choice.route.label}
          className="max-w-[16ch] shrink-0 truncate text-[10px] text-muted-foreground-faint"
        >
          {choice.route.label}
        </span>
      )}
    </button>
  )
}

export function ModelPicker({
  value,
  providers,
  onChange,
  role,
  variant = 'inline',
  dropUp,
  reasoningEffort,
  onReasoningEffortChange
}: ModelPickerProps): React.ReactNode {
  const { t } = useTranslation()
  const [isDropdownOpen, setIsDropdownOpen] = useState(false)
  const [menuPos, setMenuPos] = useState({ top: 0, left: 0, width: 0 })
  const [searchQuery, setSearchQuery] = useState('')
  const [debouncedQuery, setDebouncedQuery] = useState('')
  const [recentModelIds, setRecentModelIds] = useState<string[]>(() => getRecentModelIds(role))
  const dropdownRef = useRef<HTMLDivElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const c = roleClasses(role)

  // The model list lives in a portal so modal scroll containers never clip
  // it: it is anchored to the trigger and flips up when space runs out.
  const placeMenu = useCallback(() => {
    const trigger = triggerRef.current
    const menu = menuRef.current
    if (!trigger || !menu) return
    const rect = trigger.getBoundingClientRect()
    const gap = 4
    const height = menu.offsetHeight
    const fitsBelow = rect.bottom + gap + height <= window.innerHeight - 8
    const fitsAbove = rect.top - gap - height >= 8
    const openUp =
      dropUp ?? (!fitsBelow && (fitsAbove || rect.top > window.innerHeight - rect.bottom))
    setMenuPos({
      top: openUp ? Math.max(8, rect.top - gap - height) : rect.bottom + gap,
      left: Math.max(8, Math.min(rect.left, window.innerWidth - rect.width - 8)),
      width: rect.width
    })
  }, [dropUp])

  useEffect(() => {
    if (!isDropdownOpen) return
    const handler = (e: MouseEvent): void => {
      const target = e.target as Node
      if (menuRef.current?.contains(target) || dropdownRef.current?.contains(target)) return
      setIsDropdownOpen(false)
      setSearchQuery('')
    }
    const handleViewport = (): void => placeMenu()
    document.addEventListener('mousedown', handler)
    window.addEventListener('resize', handleViewport)
    window.addEventListener('scroll', handleViewport, true)
    return () => {
      document.removeEventListener('mousedown', handler)
      window.removeEventListener('resize', handleViewport)
      window.removeEventListener('scroll', handleViewport, true)
    }
  }, [isDropdownOpen, placeMenu])

  useEffect(() => {
    if (isDropdownOpen && inputRef.current) inputRef.current.focus()
  }, [isDropdownOpen])

  // The list filters on the settled query so fast typing never re-renders it
  // per keystroke.
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedQuery(searchQuery), 100)
    return () => clearTimeout(timer)
  }, [searchQuery])

  const runnable = useMemo(() => providers.filter(isSelectableForPairExecution), [providers])
  // Endpoints with models but no usable account are kept out of the runnable
  // list, yet still listed: they explain why a remembered model is missing.
  const blocked = useMemo(
    () =>
      providers.filter(
        (provider) => !isSelectableForPairExecution(provider) && provider.models.length > 0
      ),
    [providers]
  )
  const runnableGroups = useMemo(() => endpointGroupsFor(runnable), [runnable])
  const blockedGroups = useMemo(() => endpointGroupsFor(blocked), [blocked])

  // The whole registry resolves the stored value and the recent history, even
  // when the endpoint that would run them is not signed in.
  const canonicalModels = useMemo(() => buildCanonicalModels(providers), [providers])

  // A modal opened before the parent resolved a default still shows the model
  // this role used last, so the trigger never reads as empty.
  const effectiveValue = value || getPreferredModelId(role)
  const resolved = useMemo(
    () => resolveSelection(canonicalModels, effectiveValue, reasoningEffort),
    [canonicalModels, effectiveValue, reasoningEffort]
  )
  const currentModel = resolved.model
  const currentEffort = resolved.effort
  // The endpoint that runs the selection: the one this role last used for the
  // model, so a remembered route survives re-opening the picker.
  const currentRoute = useMemo(
    () => (currentModel ? pickDefaultRoute(currentModel, role) : undefined),
    [currentModel, role]
  )

  const runnableEndpoints = useMemo(
    () => runnableGroups.flatMap((entry) => entry.choices.map((choice) => choice.endpoint)),
    [runnableGroups]
  )
  const activeEndpoint = useMemo(() => {
    const remembered = runnableEndpoints.find(
      (endpoint) => endpoint.key === currentRoute?.providerId
    )
    return remembered ?? runnableEndpoints[0]
  }, [runnableEndpoints, currentRoute])

  // Step two lists the active endpoint's own catalog: the backend resolves the
  // stored id onto (endpoint, account, model) at turn time.
  const activeProvider = useMemo(
    () => runnable.find((provider) => provider.id === activeEndpoint?.key),
    [runnable, activeEndpoint]
  )

  const choiceModels = useMemo(
    () => (activeProvider ? choiceModelsFor(activeProvider) : []),
    [activeProvider]
  )

  const recentChoices = useMemo(() => {
    const choices: RecentChoice[] = []
    for (const storedId of recentModelIds) {
      const selection = resolveSelection(canonicalModels, storedId)
      // An id the registry no longer lists has nothing to show and picking it
      // would write an unknown model back into the pair.
      if (selection.model) {
        choices.push({ storedId, model: selection.model, route: selection.route })
      }
    }
    return choices.slice(0, MAX_RECENT_MODELS)
  }, [recentModelIds, canonicalModels])

  const filteredModels = useMemo(() => {
    if (!debouncedQuery.trim()) return choiceModels
    return choiceModels.filter((model) => modelMatchesQuery(model, debouncedQuery))
  }, [choiceModels, debouncedQuery])

  // Re-anchor after the result count changes: the menu height follows it.
  useLayoutEffect(() => {
    if (isDropdownOpen) placeMenu()
  }, [isDropdownOpen, placeMenu, filteredModels.length])

  const applyLeaf = (
    modelId: string,
    reasoningEffortValue: string | undefined,
    canonicalKey: string,
    route: EndpointRoute | undefined
  ): void => {
    // The stored list and the rendered one move together: the picker must show
    // the model just picked, and switching endpoint back must land on it.
    setRecentModelIds(saveRecentModelId(role, modelId))
    savePreferredModelId(role, modelId)
    if (route) {
      saveLastRouteKey(role, canonicalKey, route.key)
      // `onChange` only carries the model id, and two endpoints can list the
      // same one. The backend is told which endpoint this pick was made under
      // before the id reaches it, so the turn talks to that endpoint.
      reportPickedRoute(role, modelId, route.providerId)
    }
    onChange(modelId)
    onReasoningEffortChange?.(reasoningEffortValue)
  }

  const openDropdown = (): void => {
    setSearchQuery('')
    setIsDropdownOpen(true)
  }

  const closeDropdown = (): void => {
    setIsDropdownOpen(false)
    setSearchQuery('')
  }

  const openModelsSection = (): void => {
    closeDropdown()
    void emit('app:menu:go-models').catch(() => {
      // No Tauri event bridge (plain browser, unit tests) — nothing to navigate.
    })
  }

  const selectModel = (model: CanonicalModel): void => {
    const route = pickDefaultRoute(model, role)
    if (!route) return
    const leaf = defaultLeafForRoute(route)
    applyLeaf(leaf.modelId, leaf.reasoningEffort, model.canonicalKey, route)
    closeDropdown()
  }

  // Switching endpoint lands on its most recent model for this role, else its
  // first choice, then opens the model list so step two is one click away.
  const selectEndpoint = (endpoint: EndpointOption): void => {
    if (endpoint.key === activeEndpoint?.key) {
      if (isDropdownOpen) closeDropdown()
      else openDropdown()
      return
    }
    const provider = runnable.find((candidate) => candidate.id === endpoint.key)
    if (!provider) return
    const choices = choiceModelsFor(provider)
    const remembered = recentModelIds
      .map((id) => resolveSelection(canonicalFor(provider), id))
      .find((selection) => selection.model)
    const model = remembered?.model ?? choices.find((candidate) => candidate.ready) ?? choices[0]
    const route = model ? pickDefaultRoute(model, role) : undefined
    if (!model || !route) return
    const leaf = defaultLeafForRoute(route)
    applyLeaf(leaf.modelId, leaf.reasoningEffort, model.canonicalKey, route)
    openDropdown()
  }

  const selectRecent = (choice: RecentChoice): void => {
    const route = choice.route ?? pickDefaultRoute(choice.model, role)
    // A model whose endpoints have all disappeared keeps its stored id: the
    // picker must not rewrite an id the registry can no longer confirm.
    const modelId = route ? route.modelId : choice.storedId
    const effort = route ? defaultLeafForRoute(route).reasoningEffort : undefined
    applyLeaf(modelId, effort, choice.model.canonicalKey, route)
    closeDropdown()
  }

  const selectEffort = (option: EffortOption): void => {
    if (!currentModel || !currentRoute) return
    applyLeaf(currentRoute.modelId, option.value, currentModel.canonicalKey, currentRoute)
  }

  const resetReasoning = (): void => {
    if (!currentModel || !currentRoute) return
    const leaf = defaultLeafForRoute(currentRoute)
    applyLeaf(leaf.modelId, leaf.reasoningEffort, currentModel.canonicalKey, currentRoute)
  }

  const effortLabel = (level: string): string => {
    const key = effortTranslationKey(level)
    return key ? t(key).toLowerCase() : level.toLowerCase()
  }

  const effortOptions = useMemo<EffortOption[]>(
    () => (currentRoute?.efforts ?? []).map((value) => ({ value })),
    [currentRoute]
  )
  const showEffort = Boolean(currentModel && currentRoute && effortOptions.length > 0)

  // Escape anywhere inside an open dropdown closes just the dropdown — it must
  // not reach an enclosing modal's Escape handler and discard the modal draft.
  const handleDropdownKeyDown = (e: React.KeyboardEvent<HTMLDivElement>): void => {
    if (!isDropdownOpen || e.key !== 'Escape' || isImeKeyEvent(e)) return
    e.preventDefault()
    e.stopPropagation()
    closeDropdown()
  }

  const handleSearchKeyDown = (e: React.KeyboardEvent<HTMLInputElement>): void => {
    if (e.key !== 'Enter') return
    // Never let Enter in the search box submit the enclosing modal form — not
    // even the Enter that confirms an IME candidate (WebKit reports it as Enter).
    e.preventDefault()
    e.stopPropagation()
    if (isImeKeyEvent(e)) return
    if (filteredModels.length === 1) selectModel(filteredModels[0])
  }

  const pickerContent = (
    <>
      {/* Step 1 — the endpoint that runs the model */}
      {runnableGroups.length > 0 && (
        <div className="space-y-1">
          <div className="flex items-baseline gap-2">
            <span className={fieldLabel}>{t('pickers.endpoint')}</span>
            {activeEndpoint && (
              <span className={cn('truncate text-[10px] uppercase tracking-[0.14em]', c.fg)}>
                · {activeEndpoint.label}
              </span>
            )}
          </div>
          <div
            role="radiogroup"
            aria-label={t('pickers.endpoint')}
            className="max-h-40 space-y-px overflow-y-auto scrollbar-thin"
          >
            {runnableGroups.map(({ group, choices }) => (
              <div key={group.vendor} className="space-y-px">
                {runnableGroups.length > 1 && (
                  <div className={groupHeader}>── {group.label} ──</div>
                )}
                {choices.map((choice) => (
                  <EndpointRow
                    key={choice.endpoint.key}
                    choice={choice}
                    active={choice.endpoint.key === activeEndpoint?.key}
                    role={role}
                    onSelect={selectEndpoint}
                  />
                ))}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Step 2 — a model the active endpoint lists */}
      <div className="flex flex-col gap-2">
        <span className={cn(fieldLabel, 'font-sans')}>{t('pickers.model')}</span>
        <div className="relative" ref={dropdownRef} onKeyDown={handleDropdownKeyDown}>
          <button
            ref={triggerRef}
            type="button"
            onClick={() => (isDropdownOpen ? closeDropdown() : openDropdown())}
            aria-haspopup="listbox"
            aria-expanded={isDropdownOpen}
            className={cn(
              'flex w-full cursor-pointer items-center gap-2 rounded-sm border bg-background px-2 py-1.5 text-left text-[12px] transition-colors',
              currentModel ? c.border : 'border-border hover:border-foreground/40'
            )}
          >
            {currentModel ? (
              <div className="flex min-w-0 flex-1 items-center gap-2">
                <VendorIcon canonicalKey={currentModel.canonicalKey} />
                <span
                  title={currentModel.displayName}
                  className="min-w-0 flex-1 line-clamp-2 text-foreground/90 [overflow-wrap:anywhere]"
                >
                  {currentModel.displayName}
                </span>
                {currentRoute && (
                  <span
                    title={currentRoute.label}
                    className="max-w-[16ch] shrink-0 truncate text-[10px] text-muted-foreground-faint"
                  >
                    · {currentRoute.label}
                  </span>
                )}
              </div>
            ) : (
              <div className="flex min-w-0 flex-1 items-center gap-2 text-muted-foreground">
                <Search size={11} className="shrink-0 translate-y-[1px]" />
                <span>
                  {recentChoices.length > 0 ? t('pickers.allModels') : t('pickers.selectModel')}
                </span>
              </div>
            )}
            <ChevronDown
              size={12}
              className={cn(
                'shrink-0 text-muted-foreground-faint transition-transform',
                isDropdownOpen && 'rotate-180'
              )}
            />
          </button>

          {isDropdownOpen &&
            createPortal(
              <div
                ref={menuRef}
                onKeyDown={handleDropdownKeyDown}
                className={cn('fixed overflow-hidden', Z.popover, popoverSurface)}
                style={{ top: menuPos.top, left: menuPos.left, width: menuPos.width }}
              >
                <div className="border-b border-border p-1.5">
                  <div className="relative">
                    <Search
                      size={11}
                      className="absolute left-2 top-1/2 -translate-y-1/2 text-muted-foreground-faint"
                    />
                    <input
                      ref={inputRef}
                      type="text"
                      value={searchQuery}
                      aria-label={t('pickers.searchModels')}
                      onChange={(e) => setSearchQuery(e.target.value)}
                      onKeyDown={handleSearchKeyDown}
                      placeholder={t('pickers.searchModels')}
                      className={searchInput}
                    />
                  </div>
                </div>
                <div className="max-h-60 overflow-y-auto scrollbar-thin p-1">
                  {filteredModels.length === 0 ? (
                    <div className={emptyHint}>— {t('pickers.noModels')} —</div>
                  ) : (
                    <div role="listbox" className="space-y-px">
                      {filteredModels.map((model) => {
                        const route = model.routes[0]
                        const isActiveModel = model.canonicalKey === currentModel?.canonicalKey
                        return (
                          <button
                            key={model.canonicalKey}
                            type="button"
                            role="option"
                            aria-selected={isActiveModel}
                            onClick={() => selectModel(model)}
                            title={model.displayName}
                            className={menuItem(isActiveModel)}
                          >
                            <span
                              aria-hidden
                              className={cn(
                                'shrink-0',
                                isActiveModel ? c.fg : 'text-muted-foreground-faint'
                              )}
                            >
                              {isActiveModel ? '●' : '○'}
                            </span>
                            <VendorIcon canonicalKey={model.canonicalKey} />
                            {/* No title here: the option button already carries
                                it; a duplicate breaks getByTitle queries. */}
                            <span className="min-w-0 flex-1 truncate text-foreground/90 [overflow-wrap:anywhere]">
                              {model.displayName}
                            </span>
                            {route && (
                              <span className="max-w-[20ch] shrink-0 truncate font-mono text-[10px] text-muted-foreground-faint">
                                {route.modelId}
                              </span>
                            )}
                          </button>
                        )
                      })}
                    </div>
                  )}
                </div>

                {/* Endpoints whose credential needs attention — the reason a
                  remembered model is missing from the list above. */}
                {!searchQuery.trim() && blockedGroups.length > 0 && (
                  <div className="space-y-px border-t border-border/50 p-1.5">
                    <div className={groupHeader}>── {t('pickers.needsCredential')} ──</div>
                    {blockedGroups.map(({ choices }) =>
                      choices.map((choice) => (
                        <BlockedEndpointRow
                          key={choice.endpoint.key}
                          provider={choice.provider}
                          onOpenModels={openModelsSection}
                        />
                      ))
                    )}
                  </div>
                )}
              </div>,
              document.body
            )}
        </div>

        {/* Step 3 — reasoning effort, hidden when the endpoint offers none */}
        {showEffort && currentModel && currentRoute && (
          <div className="space-y-1">
            <div className="flex items-baseline gap-2">
              <span className={fieldLabel}>{t('pickers.reasoning')}</span>
              {currentEffort && (
                <span className={cn('text-[10px] uppercase tracking-[0.14em]', c.fg)}>
                  · {effortLabel(currentEffort.value)}
                </span>
              )}
              {currentEffort && (
                <GlassButton
                  variant="ghost"
                  size="xs"
                  onClick={resetReasoning}
                  className="ml-auto text-muted-foreground"
                >
                  {t('pickers.resetReasoning')}
                </GlassButton>
              )}
            </div>
            <div
              role="radiogroup"
              aria-label={t('pickers.reasoning')}
              className="flex flex-1 overflow-hidden rounded-md border border-border"
            >
              {effortOptions.map((option) => {
                const isActive = option.value === currentEffort?.value
                return (
                  <button
                    key={option.value}
                    type="button"
                    role="radio"
                    aria-checked={isActive}
                    onClick={() => selectEffort(option)}
                    className={cn(
                      'flex-1 border-r border-border px-2 py-0.5 text-[11px] transition-colors cursor-pointer last:border-r-0',
                      isActive
                        ? cn(c.bg, c.fg, c.border)
                        : cn('text-muted-foreground hover:text-foreground', hoverTint)
                    )}
                  >
                    {effortLabel(option.value)}
                  </button>
                )
              })}
            </div>
          </div>
        )}
      </div>

      {/* Recent quick-picks */}
      {recentChoices.length > 0 && (
        <div className="space-y-1">
          <div className={groupHeader}>── {t('pickers.recent')} ──</div>
          <div role="radiogroup" aria-label={t('pickers.recent')} className="space-y-px">
            {recentChoices.map((choice) => (
              <QuickPickRow
                key={choice.storedId}
                choice={choice}
                selected={choice.model.canonicalKey === currentModel?.canonicalKey}
                role={role}
                onSelect={selectRecent}
              />
            ))}
          </div>
        </div>
      )}

      {/* Nothing signed in at all — the picker cannot offer a model. */}
      {runnableGroups.length === 0 && blockedGroups.length === 0 && (
        <div className="space-y-2 text-center">
          <p className={emptyHint}>{t('pickers.noEndpointsGuide')}</p>
          <GlassButton variant="ghost" size="xs" onClick={openModelsSection}>
            {t('pickers.openModels')}
          </GlassButton>
        </div>
      )}
    </>
  )

  if (variant === 'card') {
    return (
      <div className={cn('flex flex-col rounded-md border bg-background p-3 font-sans', c.border)}>
        <div className="mb-2 flex items-baseline gap-2">
          <span aria-hidden className={c.fg}>
            ●
          </span>
          <span className={cn('text-[11px] font-medium uppercase tracking-[0.14em]', c.fg)}>
            {role === 'mentor' ? t('pickers.mentorLabel') : t('pickers.executorLabel')}
          </span>
          <span className="text-[10px] text-muted-foreground-faint">
            · {role === 'mentor' ? t('pickers.mentorDesc') : t('pickers.executorDesc')}
          </span>
        </div>
        <div className="flex flex-col gap-2">{pickerContent}</div>
      </div>
    )
  }

  return pickerContent
}
