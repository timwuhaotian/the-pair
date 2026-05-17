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
import { ReasoningEffortPicker } from './ReasoningEffortPicker'

const RECENT_MODELS_KEY_PREFIX = 'the-pair-recent-models-'
const MAX_RECENT_MODELS = 4

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

function roleClasses(role: 'mentor' | 'executor'): {
  fg: string
  border: string
  bg: string
} {
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
      <span className="min-w-0 flex-1 truncate text-foreground/90">{model.displayName}</span>
      <span className="shrink-0 text-[10px] text-muted-foreground-faint truncate max-w-[10ch]">
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

  const readyModels = useMemo(
    () => models.filter((model) => isSelectableForPairExecution(model)),
    [models]
  )

  const recentModels = useMemo(() => {
    return recentModelIds
      .map((id) => readyModels.find((model) => getQualifiedModel(model) === id))
      .filter((model): model is AvailableModel => model !== undefined)
      .slice(0, MAX_RECENT_MODELS)
  }, [recentModelIds, readyModels])

  const filteredDropdownModels = useMemo(() => {
    const selectable = models.filter((m) => isSelectableForPairExecution(m))
    if (!searchQuery.trim()) return selectable
    const query = searchQuery.toLowerCase().replace(/[\s.-]/g, '')
    const fuzzyMatch = (text: string, search: string): boolean => {
      const normalized = text.toLowerCase().replace(/[\s.-]/g, '')
      let searchIndex = 0
      for (let i = 0; i < normalized.length && searchIndex < search.length; i++) {
        if (normalized[i] === search[searchIndex]) searchIndex++
      }
      return searchIndex === search.length
    }
    return selectable.filter(
      (model) =>
        fuzzyMatch(model.displayName, query) ||
        fuzzyMatch(model.providerLabel, query) ||
        fuzzyMatch(model.modelId, query)
    )
  }, [models, searchQuery])

  const handleSelect = (model: AvailableModel): void => {
    if (!isSelectableForPairExecution(model)) return
    const modelId = getQualifiedModel(model)
    saveRecentModelId(role, modelId)
    savePreferredModelId(role, modelId)
    onChange(modelId)
    setIsDropdownOpen(false)
    setSearchQuery('')

    const levels = model.reasoningEffortLevels
    if (levels && levels.length > 0 && onReasoningEffortChange) {
      const defaultLevel = levels.includes('medium') ? 'medium' : levels[0]
      onReasoningEffortChange(defaultLevel)
    } else if (onReasoningEffortChange) {
      onReasoningEffortChange(undefined)
    }
  }

  const selectedModel = useMemo(
    () => models.find((model) => getQualifiedModel(model) === value),
    [models, value]
  )
  const isRecentSelection = recentModels.some((model) => getQualifiedModel(model) === value)

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
                onSelect={handleSelect}
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
              selectedModel && !isRecentSelection
                ? c.border
                : 'border-border hover:border-foreground/40'
            )}
          >
            {selectedModel && !isRecentSelection ? (
              <div className="flex min-w-0 flex-1 items-baseline gap-2">
                <span aria-hidden className={c.fg}>
                  ●
                </span>
                <span className="truncate text-foreground/90">{selectedModel.displayName}</span>
                <span className="shrink-0 text-[10px] text-muted-foreground-faint">
                  · {selectedModel.providerLabel}
                </span>
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
                {filteredDropdownModels.length === 0 ? (
                  <div className="py-3 text-center text-[11px] text-muted-foreground">
                    — {t('pickers.noModels')} —
                  </div>
                ) : (
                  <div className="space-y-px">
                    {filteredDropdownModels.map((model) => {
                      const selected = getQualifiedModel(model) === value
                      const showSourceProvider =
                        model.sourceProviderLabel &&
                        model.sourceProviderLabel !== model.providerLabel
                      return (
                        <button
                          key={getQualifiedModel(model)}
                          type="button"
                          onClick={() => handleSelect(model)}
                          title={`${model.displayName} · ${
                            showSourceProvider ? `${model.sourceProviderLabel} → ` : ''
                          }${model.providerLabel}${
                            model.planLabel && model.planLabel !== 'BYOK'
                              ? ` [${model.planLabel}]`
                              : ''
                          }`}
                          className={cn(
                            'flex w-full items-start gap-2 px-2 py-1 text-left rounded-sm transition-colors cursor-pointer font-mono text-[11px]',
                            selected ? cn(c.bg, c.fg) : 'hover:bg-foreground/[0.05]'
                          )}
                        >
                          <span
                            aria-hidden
                            className={cn(
                              'shrink-0 leading-[1.4]',
                              model.supportsPairExecution ? c.fg : 'state-running'
                            )}
                          >
                            {model.supportsPairExecution ? (selected ? '●' : '○') : '!'}
                          </span>
                          <span className="min-w-0 flex-1 flex flex-col gap-0.5">
                            <span className="truncate text-foreground/90">{model.displayName}</span>
                            <span className="flex items-center gap-1.5 text-[10px] text-muted-foreground-faint">
                              {showSourceProvider && (
                                <span className="role-mentor truncate">
                                  {model.sourceProviderLabel} →
                                </span>
                              )}
                              <span className="truncate">{model.providerLabel}</span>
                              {model.planLabel && model.planLabel !== 'BYOK' && (
                                <span className="shrink-0 state-done text-[9px] uppercase">
                                  [{model.planLabel}]
                                </span>
                              )}
                            </span>
                          </span>
                        </button>
                      )
                    })}
                  </div>
                )}
              </div>
            </div>
          )}
        </div>

        {selectedModel?.reasoningEffortLevels &&
          selectedModel.reasoningEffortLevels.length > 0 &&
          onReasoningEffortChange && (
            <ReasoningEffortPicker
              levels={selectedModel.reasoningEffortLevels}
              value={reasoningEffort}
              onChange={onReasoningEffortChange}
              role={role}
            />
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
