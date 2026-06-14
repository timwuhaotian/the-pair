import React, { useMemo, useState, useRef, useEffect } from 'react'
import { ChevronDown, Search } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { cn } from '../lib/utils'
import type { AvailableModel } from '../types'
import {
  getQualifiedModel,
  isSelectableForPairExecution,
  savePreferredModelId
} from '../lib/modelPreferences'
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

const RECENT_MODELS_KEY_PREFIX = 'the-pair-recent-models-'
const MAX_RECENT_MODELS = 4
const EFFORT_FALLBACK_LABELS: Record<string, string> = {
  low: 'fast',
  medium: 'balanced',
  high: 'deep'
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
        'flex w-full items-baseline gap-2 border px-2 py-1 text-left rounded-sm transition-colors cursor-pointer font-mono text-[11px]',
        selected
          ? cn(c.border, c.bg, c.fg)
          : 'border-border hover:bg-foreground/[0.05] hover:border-foreground/40'
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
  dropUp = false,
  reasoningEffort,
  onReasoningEffortChange
}: ModelPickerProps): React.ReactNode {
  const { t } = useTranslation()
  const [isDropdownOpen, setIsDropdownOpen] = useState(false)
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

  const canonicalModels = useMemo(() => buildCanonicalModels(selectableModels), [selectableModels])

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
    if (!searchQuery.trim()) return canonicalModels
    return canonicalModels.filter((model) => modelMatchesQuery(model, searchQuery))
  }, [canonicalModels, searchQuery])

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
    const key = `pickers.reasoning${level.charAt(0).toUpperCase() + level.slice(1)}` as const
    const translated = t(key)
    return translated !== key
      ? translated.toLowerCase()
      : (EFFORT_FALLBACK_LABELS[level] ?? level.toLowerCase())
  }

  const activeRoute = current.route
  const showEffort = Boolean(activeRoute && activeRoute.effortOptions.length > 0)

  const pickerContent = (
    <>
      {/* Recent quick-picks */}
      {recentModels.length > 0 && (
        <div className="space-y-1">
          <div className="text-[10px] uppercase tracking-[0.18em] text-muted-foreground">
            <span className="text-primary">──</span> {t('pickers.recent')}{' '}
            <span className="text-primary">──</span>
          </div>
          <div className="space-y-px">
            {recentModels.map((model) => (
              <QuickPickRow
                key={getQualifiedModel(model)}
                model={model}
                selected={getQualifiedModel(model) === value}
                role={role}
                onSelect={selectRecent}
              />
            ))}
          </div>
        </div>
      )}

      <div className="flex flex-col gap-2">
        <div className="relative" ref={dropdownRef}>
          <button
            type="button"
            onClick={() => setIsDropdownOpen(!isDropdownOpen)}
            aria-expanded={isDropdownOpen}
            className={cn(
              'flex w-full items-center gap-2 border px-2 py-1.5 text-left rounded-sm transition-colors cursor-pointer font-mono text-[12px]',
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
                'absolute left-0 right-0 z-50 overflow-hidden border border-border bg-popover rounded-sm',
                dropUp ? 'bottom-full mb-1' : 'top-full mt-1'
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
                    onKeyDown={(e) => {
                      if (e.key === 'Escape') {
                        setIsDropdownOpen(false)
                        setSearchQuery('')
                      }
                    }}
                    placeholder={t('pickers.searchModels')}
                    className="w-full border border-border bg-background py-1 pl-7 pr-2 text-[12px] text-foreground placeholder:text-muted-foreground-faint focus:border-foreground/40 focus:outline-none rounded-sm font-mono"
                  />
                </div>
              </div>
              <div className="max-h-60 overflow-y-auto scrollbar-thin p-1">
                {filteredModels.length === 0 ? (
                  <div className="py-3 text-center text-[11px] text-muted-foreground">
                    — {t('pickers.noModels')} —
                  </div>
                ) : (
                  <div className="space-y-px">
                    {filteredModels.map((model) => {
                      const isActiveModel = model.canonicalKey === current.model?.canonicalKey
                      const multiRoute = model.routes.length > 1
                      return (
                        <div key={model.canonicalKey}>
                          <button
                            type="button"
                            onClick={() => {
                              if (isActiveModel && multiRoute) return
                              selectModel(model)
                            }}
                            title={model.displayName}
                            className={cn(
                              'flex w-full items-center gap-2 px-2 py-1 text-left rounded-sm transition-colors cursor-pointer font-mono text-[11px]',
                              isActiveModel ? cn(c.bg, c.fg) : 'hover:bg-foreground/[0.05]'
                            )}
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
                              <span className="shrink-0 text-[9px] uppercase tracking-wide text-muted-foreground-faint">
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
                            <div className="ml-3 mt-px mb-1 flex flex-col gap-px border-l border-border pl-2">
                              {model.routes.map((route) => {
                                const selectedRoute = route.key === current.route?.key
                                return (
                                  <button
                                    key={route.key}
                                    type="button"
                                    onClick={() => selectRoute(model, route)}
                                    title={route.accessLabel}
                                    className={cn(
                                      'flex w-full items-baseline gap-2 px-2 py-1 text-left rounded-sm transition-colors cursor-pointer font-mono text-[10px]',
                                      selectedRoute
                                        ? c.fg
                                        : 'text-muted-foreground hover:bg-foreground/[0.05]'
                                    )}
                                  >
                                    <span aria-hidden className="shrink-0">
                                      {selectedRoute ? '●' : '○'}
                                    </span>
                                    <span className="min-w-0 flex-1 truncate text-foreground/80">
                                      {route.providerLabel}
                                    </span>
                                    <span className="shrink-0 text-[9px] text-muted-foreground-faint truncate max-w-[16ch]">
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
            </div>
          )}
        </div>

        {/* Reasoning effort — sourced from the selected route, hidden when it has none */}
        {showEffort && activeRoute && current.model && (
          <div className="space-y-1 font-mono">
            <div className="flex items-baseline gap-2">
              <span className={cn('text-[10px] uppercase tracking-[0.16em]', c.fg)}>
                {t('pickers.reasoning')}
              </span>
              {current.effort && (
                <span className={cn('text-[10px] uppercase tracking-[0.14em]', c.fg)}>
                  · {effortLabel(current.effort.value)}
                </span>
              )}
            </div>
            <div className="flex flex-1 border border-border rounded-sm overflow-hidden">
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
                        : 'text-muted-foreground hover:bg-foreground/[0.05] hover:text-foreground'
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
        className={cn('flex flex-col border rounded-sm p-3 font-mono', c.border, 'bg-background')}
      >
        <div className="mb-2 flex items-baseline gap-2">
          <span aria-hidden className={c.fg}>
            ●
          </span>
          <span className={cn('text-[11px] uppercase tracking-[0.16em] font-bold', c.fg)}>
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
