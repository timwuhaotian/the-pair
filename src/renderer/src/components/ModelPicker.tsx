import React, { useMemo, useState, useRef, useEffect } from 'react'
import { ChevronDown, Search } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import {
  Z,
  emptyHint,
  fieldLabel,
  hoverTint,
  groupHeader,
  menuItem,
  popoverSurface,
  searchInput
} from '../lib/ui'
import { GlassButton } from './ui/GlassButton'
import type { AvailableModel, ProviderKind } from '../types'
import {
  getQualifiedModel,
  isSelectableForPairExecution,
  savePreferredModelId
} from '../lib/modelPreferences'
import { PROVIDER_LOGIN_COMMANDS } from '../lib/providerSetup'
import { modelMatchesId } from '../lib/providerResolution'
import {
  buildCanonicalModels,
  defaultLeafForRoute,
  modelMatchesQuery,
  pickDefaultRoute,
  resolveSelection,
  saveLastRouteKey,
  type CanonicalModel,
  type EffortOption,
  type ModelRoute
} from '../lib/modelCatalogGrouping'
import { ordinaryModelChoices, preferMiniMaxFlash } from '../lib/minimaxModelChoices'
import { isImeKeyEvent } from './keyboard'

const RECENT_MODELS_KEY_PREFIX = 'the-pair-recent-models-'
const MAX_RECENT_MODELS = 4

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

function saveRecentModelId(role: 'mentor' | 'executor', modelId: string): void {
  const key = RECENT_MODELS_KEY_PREFIX + role
  const recent = getRecentModelIds(role).filter((id) => id !== modelId)
  recent.unshift(modelId)
  localStorage.setItem(key, JSON.stringify(recent.slice(0, MAX_RECENT_MODELS)))
}

export interface ModelPickerProps {
  value: string
  models: AvailableModel[]
  onChange: (value: string) => void
  role: 'mentor' | 'executor'
  variant?: 'card' | 'inline'
  dropUp?: boolean
  reasoningEffort?: string
  onReasoningEffortChange?: (value: string | undefined) => void
}

function roleClasses(role: 'mentor' | 'executor'): { fg: string; border: string; bg: string } {
  if (role === 'mentor')
    return { fg: 'role-mentor', border: 'border-role-mentor', bg: 'bg-role-mentor' }
  return { fg: 'role-executor', border: 'border-role-executor', bg: 'bg-role-executor' }
}

function QuickPickRow({
  model,
  selected,
  role,
  onSelect
}: {
  model: AvailableModel
  selected: boolean
  role: 'mentor' | 'executor'
  onSelect: (model: AvailableModel) => void
}): React.ReactNode {
  const c = roleClasses(role)
  return (
    <button
      type="button"
      onClick={() => onSelect(model)}
      className={cn(
        'flex w-full items-baseline gap-2 border px-2 py-1 text-left rounded-md transition-colors cursor-pointer font-mono text-[11px]',
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
      <span className="min-w-0 flex-1 truncate text-foreground/90">
        {model.canonicalDisplayName?.trim() || model.displayName}
      </span>
      <span className="shrink-0 text-[10px] text-muted-foreground-faint truncate max-w-[12ch]">
        {model.providerLabel}
      </span>
    </button>
  )
}

export function ModelPicker({
  value,
  models,
  onChange,
  role,
  variant = 'inline',
  dropUp,
  reasoningEffort,
  onReasoningEffortChange
}: ModelPickerProps): React.ReactNode {
  const { t } = useTranslation()
  const [isDropdownOpen, setIsDropdownOpen] = useState(false)
  const [autoDropUp, setAutoDropUp] = useState(false)
  // Flip up when the trigger sits in the lower half of the viewport and the
  // caller didn't pin a direction: the list lives inside overflow-clipped
  // modals/panels, so opening downward there hides most of it.
  const effectiveDropUp = dropUp ?? autoDropUp
  const [searchQuery, setSearchQuery] = useState('')
  const [recentModelIds] = useState<string[]>(() => getRecentModelIds(role))
  const dropdownRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const c = roleClasses(role)

  useEffect(() => {
    if (!isDropdownOpen) return
    const handler = (e: MouseEvent): void => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setIsDropdownOpen(false)
        setSearchQuery('')
      }
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [isDropdownOpen])

  useEffect(() => {
    if (isDropdownOpen && inputRef.current) inputRef.current.focus()
  }, [isDropdownOpen])

  const selectableModels = useMemo(
    () => models.filter((model) => isSelectableForPairExecution(model)),
    [models]
  )

  // Keep the full catalog for saved selections and recent history.
  const canonicalModels = useMemo(() => buildCanonicalModels(selectableModels), [selectableModels])
  const choiceModels = useMemo(
    () =>
      preferMiniMaxFlash(
        buildCanonicalModels(ordinaryModelChoices(selectableModels)),
        (model) =>
          model.routes.find((route) => route.planLabel !== 'custom-provider')?.baseQualifiedId ?? ''
      ),
    [selectableModels]
  )

  const current = useMemo(
    () => resolveSelection(canonicalModels, value, reasoningEffort),
    [canonicalModels, value, reasoningEffort]
  )

  const recentModels = useMemo(() => {
    return recentModelIds
      .map((id) => selectableModels.find((model) => getQualifiedModel(model) === id))
      .filter((model): model is AvailableModel => model !== undefined)
      .slice(0, MAX_RECENT_MODELS)
  }, [recentModelIds, selectableModels])

  const filteredModels = useMemo(() => {
    if (!searchQuery.trim()) return choiceModels
    return choiceModels.filter((model) => modelMatchesQuery(model, searchQuery))
  }, [choiceModels, searchQuery])

  // Models that are auth-missing (CLI installed but not signed in). We show a hint
  // so the user knows there are more models available after authenticating.
  const authMissingProviders = useMemo((): Array<{
    kind: ProviderKind
    label: string
    loginCommand?: string
    count: number
  }> => {
    const byProvider = new Map<ProviderKind, { label: string; count: number }>()
    for (const model of models) {
      if (model.availabilityStatus === 'auth-missing') {
        const existing = byProvider.get(model.provider)
        if (existing) {
          existing.count++
        } else {
          byProvider.set(model.provider, { label: model.providerLabel, count: 1 })
        }
      }
    }
    return Array.from(byProvider.entries()).map(([kind, info]) => ({
      kind,
      label: info.label,
      loginCommand: PROVIDER_LOGIN_COMMANDS[kind],
      count: info.count
    }))
  }, [models])

  const applyLeaf = (
    qualifiedId: string,
    reasoningEffortValue: string | undefined,
    canonicalKey: string,
    routeKey: string
  ): void => {
    saveRecentModelId(role, qualifiedId)
    savePreferredModelId(role, qualifiedId)
    saveLastRouteKey(role, canonicalKey, routeKey)
    onChange(qualifiedId)
    onReasoningEffortChange?.(reasoningEffortValue)
  }

  const handleProviderSignIn = async (providerKind: string): Promise<void> => {
    try {
      await window.api?.config?.launchLogin?.(providerKind)
    } catch {
      // silent — the onboarding screen handles error display
    }
  }

  const selectModel = (model: CanonicalModel): void => {
    const route = pickDefaultRoute(model, role)
    if (!route) return
    const leaf = defaultLeafForRoute(route)
    applyLeaf(leaf.qualifiedId, leaf.reasoningEffort, model.canonicalKey, route.key)
    // Single-route models have nothing left to choose; multi-route models stay open so
    // the route sub-picker can be refined.
    if (model.routes.length <= 1) {
      setIsDropdownOpen(false)
      setSearchQuery('')
    }
  }

  const selectRoute = (model: CanonicalModel, route: ModelRoute): void => {
    const leaf = defaultLeafForRoute(route)
    applyLeaf(leaf.qualifiedId, leaf.reasoningEffort, model.canonicalKey, route.key)
    setIsDropdownOpen(false)
    setSearchQuery('')
  }

  const selectEffort = (model: CanonicalModel, route: ModelRoute, option: EffortOption): void => {
    applyLeaf(option.qualifiedId, option.reasoningEffort, model.canonicalKey, route.key)
  }

  const resetReasoning = (model: CanonicalModel, route: ModelRoute): void => {
    applyLeaf(route.baseQualifiedId, undefined, model.canonicalKey, route.key)
  }

  const selectRecent = (model: AvailableModel): void => {
    const qualifiedId = getQualifiedModel(model)
    const sel = resolveSelection(canonicalModels, qualifiedId)
    if (sel.model && sel.route) {
      const leaf = sel.effort
        ? { qualifiedId: sel.effort.qualifiedId, reasoningEffort: sel.effort.reasoningEffort }
        : defaultLeafForRoute(sel.route)
      applyLeaf(leaf.qualifiedId, leaf.reasoningEffort, sel.model.canonicalKey, sel.route.key)
    } else {
      applyLeaf(qualifiedId, undefined, model.canonicalKey ?? '', '')
    }
    setIsDropdownOpen(false)
    setSearchQuery('')
  }

  const effortLabel = (level: string): string => {
    const key = effortTranslationKey(level)
    return key ? t(key).toLowerCase() : level.toLowerCase()
  }

  const activeRoute = current.route
  const showEffort = Boolean(activeRoute && activeRoute.effortOptions.length > 0)

  const closeDropdown = (): void => {
    setIsDropdownOpen(false)
    setSearchQuery('')
  }

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
      {/* Recent quick-picks */}
      {recentModels.length > 0 && (
        <div className="space-y-1">
          <div className={groupHeader}>── {t('pickers.recent')} ──</div>
          <div className="space-y-px">
            {recentModels.map((model) => (
              <QuickPickRow
                key={getQualifiedModel(model)}
                model={model}
                selected={modelMatchesId(model, value)}
                role={role}
                onSelect={selectRecent}
              />
            ))}
          </div>
        </div>
      )}

      <div className="flex flex-col gap-2">
        <div className="relative" ref={dropdownRef} onKeyDown={handleDropdownKeyDown}>
          <button
            type="button"
            onClick={(e) => {
              if (!isDropdownOpen) {
                const rect = e.currentTarget.getBoundingClientRect()
                setAutoDropUp(rect.top > window.innerHeight / 2)
              }
              setIsDropdownOpen(!isDropdownOpen)
            }}
            aria-haspopup="listbox"
            aria-expanded={isDropdownOpen}
            className={cn(
              'flex w-full cursor-pointer items-center gap-2 rounded-sm border bg-background px-2 py-1.5 text-left font-mono text-[12px] transition-colors',
              current.model ? c.border : 'border-border hover:border-foreground/40'
            )}
          >
            {current.model ? (
              <div className="flex min-w-0 flex-1 items-baseline gap-2">
                <span aria-hidden className={c.fg}>
                  ●
                </span>
                <span className="truncate text-foreground/90">{current.model.displayName}</span>
                {current.route && (
                  <span className="shrink-0 text-[10px] text-muted-foreground-faint truncate max-w-[16ch]">
                    · {current.route.providerLabel}
                  </span>
                )}
              </div>
            ) : (
              <div className="flex min-w-0 flex-1 items-baseline gap-2 text-muted-foreground">
                <Search size={11} className="shrink-0 translate-y-[1px]" />
                <span>
                  {recentModels.length > 0 ? t('pickers.allModels') : t('pickers.selectModel')}
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

          {isDropdownOpen && (
            <div
              className={cn(
                'absolute left-0 right-0 overflow-hidden',
                Z.popover,
                popoverSurface,
                effectiveDropUp ? 'bottom-full mb-1' : 'top-full mt-1'
              )}
            >
              <div className="p-1.5 border-b border-border">
                <div className="relative">
                  <Search
                    size={11}
                    className="absolute left-2 top-1/2 -translate-y-1/2 text-muted-foreground-faint"
                  />
                  <input
                    ref={inputRef}
                    type="text"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    onKeyDown={handleSearchKeyDown}
                    placeholder={t('pickers.searchModels')}
                    className={cn(searchInput, 'font-mono')}
                  />
                </div>
              </div>
              <div className="max-h-60 overflow-y-auto scrollbar-thin p-1">
                {filteredModels.length === 0 ? (
                  <div className={emptyHint}>— {t('pickers.noModels')} —</div>
                ) : (
                  <div role="listbox" className="space-y-px">
                    {filteredModels.map((model) => {
                      const isActiveModel = model.canonicalKey === current.model?.canonicalKey
                      const multiRoute = model.routes.length > 1
                      return (
                        <div key={model.canonicalKey}>
                          <button
                            type="button"
                            role="option"
                            aria-selected={isActiveModel}
                            onClick={() => {
                              if (isActiveModel && multiRoute) return
                              selectModel(model)
                            }}
                            title={model.displayName}
                            className={cn(menuItem(isActiveModel), 'font-mono')}
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
                            <span className="min-w-0 flex-1 truncate text-foreground/90">
                              {model.displayName}
                            </span>
                            {multiRoute ? (
                              <span className="shrink-0 text-[10px] uppercase tracking-wide text-muted-foreground-faint">
                                {model.routes.length} {t('pickers.routes')} ›
                              </span>
                            ) : (
                              <span className="shrink-0 text-[10px] text-muted-foreground-faint truncate max-w-[12ch]">
                                {model.routes[0]?.providerLabel}
                              </span>
                            )}
                          </button>

                          {/* Nested route sub-picker for the active multi-route model */}
                          {isActiveModel && multiRoute && (
                            <div
                              role="group"
                              className="ml-3 mt-px mb-1 flex flex-col gap-px border-l border-border pl-2"
                            >
                              {model.routes.map((route) => {
                                const selectedRoute = route.key === current.route?.key
                                return (
                                  <button
                                    key={route.key}
                                    type="button"
                                    role="option"
                                    aria-selected={selectedRoute}
                                    onClick={() => selectRoute(model, route)}
                                    title={route.accessLabel}
                                    className={cn(
                                      menuItem(selectedRoute),
                                      'items-baseline font-mono text-[11px]'
                                    )}
                                  >
                                    <span
                                      aria-hidden
                                      className={cn(
                                        'shrink-0',
                                        selectedRoute ? c.fg : 'text-muted-foreground-faint'
                                      )}
                                    >
                                      {selectedRoute ? '●' : '○'}
                                    </span>
                                    <span className="min-w-0 flex-1 truncate text-foreground/80">
                                      {route.providerLabel}
                                    </span>
                                    <span className="shrink-0 text-[10px] text-muted-foreground-faint truncate max-w-[16ch]">
                                      {route.accessLabel}
                                    </span>
                                  </button>
                                )
                              })}
                            </div>
                          )}
                        </div>
                      )
                    })}
                  </div>
                )}
              </div>

              {/* Auth-missing provider hint */}
              {!searchQuery.trim() && authMissingProviders.length > 0 && (
                <div className="border-t border-border/50 p-2 space-y-1">
                  {authMissingProviders.map((p) => (
                    <div
                      key={p.kind}
                      className="flex items-center gap-2 px-1 py-0.5 text-[10px] text-muted-foreground font-mono"
                    >
                      <span aria-hidden className="state-running">
                        ⚠
                      </span>
                      <span>
                        {p.count === 1
                          ? t('providers.moreAfterSignIn_one', {
                              count: p.count,
                              defaultValue: `${p.count} more model available after signing in`
                            })
                          : t('providers.moreAfterSignIn_other', {
                              count: p.count,
                              defaultValue: `${p.count} more models available after signing in`
                            })}{' '}
                        ({p.label})
                      </span>
                      {p.loginCommand && (
                        <button
                          type="button"
                          onClick={() => void handleProviderSignIn(p.kind)}
                          className="cursor-pointer text-primary underline-offset-2 transition-colors hover:text-primary/80 hover:underline"
                        >
                          {t('providers.signIn')}
                        </button>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        {/* Reasoning effort — sourced from the selected route, hidden when it has none */}
        {showEffort && activeRoute && current.model && (
          <div className="space-y-1 font-mono">
            <div className="flex items-baseline gap-2">
              <span className={fieldLabel}>{t('pickers.reasoning')}</span>
              {current.effort && (
                <span className={cn('text-[10px] uppercase tracking-[0.14em]', c.fg)}>
                  · {effortLabel(current.effort.value)}
                </span>
              )}
              {current.effort && (
                <GlassButton
                  variant="ghost"
                  size="xs"
                  onClick={() => resetReasoning(current.model as CanonicalModel, activeRoute)}
                  className="ml-auto text-muted-foreground"
                >
                  {t('pickers.resetReasoning')}
                </GlassButton>
              )}
            </div>
            <div className="flex flex-1 border border-border rounded-md overflow-hidden">
              {activeRoute.effortOptions.map((option) => {
                const isActive = option.value === current.effort?.value
                return (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() =>
                      selectEffort(current.model as CanonicalModel, activeRoute, option)
                    }
                    className={cn(
                      'flex-1 px-2 py-0.5 text-[11px] transition-colors cursor-pointer border-r border-border last:border-r-0',
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
    </>
  )

  if (variant === 'card') {
    return (
      <div
        className={cn('flex flex-col border rounded-md p-3 font-mono', c.border, 'bg-background')}
      >
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
