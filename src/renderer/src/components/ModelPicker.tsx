import React, { useMemo, useState, useRef, useEffect } from 'react'
import { Brain, CheckCircle2, ChevronDown, CircleAlert, Search, Zap } from 'lucide-react'
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

    // Migrate from old global key on first access
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
  /** Render as a self-contained card with role header */
  variant?: 'card' | 'inline'
  /** Open the dropdown upward instead of downward */
  dropUp?: boolean
  reasoningEffort?: string
  onReasoningEffortChange?: (value: string | undefined) => void
}

function getRoleTone(role: 'mentor' | 'executor') {
  if (role === 'mentor') {
    return {
      icon: <Brain size={14} className="text-blue-600 dark:text-blue-400" />,
      iconSm: <Brain size={12} className="text-blue-600 dark:text-blue-400" />,
      text: 'text-blue-600 dark:text-blue-400',
      border: 'border-blue-500/25',
      background: 'bg-blue-500/12 dark:bg-blue-500/14',
      ringSelected: 'ring-blue-500/30',
      bgSelected: 'bg-blue-500/25 dark:bg-blue-500/18',
      headerBg: 'bg-blue-500/10 dark:bg-blue-500/10',
      label: 'Mentor',
      subtitle: 'Analyzes, plans, reviews'
    }
  }
  return {
    icon: <Zap size={14} className="text-purple-600 dark:text-purple-400" />,
    iconSm: <Zap size={12} className="text-purple-600 dark:text-purple-400" />,
    text: 'text-purple-600 dark:text-purple-400',
    border: 'border-purple-500/25',
    background: 'bg-purple-500/12 dark:bg-purple-500/14',
    ringSelected: 'ring-purple-500/30',
    bgSelected: 'bg-purple-500/25 dark:bg-purple-500/18',
    headerBg: 'bg-purple-500/10 dark:bg-purple-500/10',
    label: 'Executor',
    subtitle: 'Writes code, runs commands'
  }
}

function QuickPickCell({
  model,
  selected,
  tone,
  onSelect
}: {
  model: AvailableModel
  selected: boolean
  tone: ReturnType<typeof getRoleTone>
  onSelect: (model: AvailableModel) => void
}) {
  return (
    <button
      type="button"
      onClick={() => onSelect(model)}
      className={cn(
        'flex w-full items-center gap-3 rounded-xl border px-3 py-2.5 text-left transition-all cursor-pointer',
        selected
          ? cn('ring-1', tone.border, tone.bgSelected, tone.ringSelected)
          : 'border-border/60 bg-muted hover:border-foreground/12 hover:bg-muted/80'
      )}
    >
      <div className="min-w-0 flex-1">
        <div className="truncate text-xs font-semibold text-foreground leading-tight">
          {model.displayName}
        </div>
        <div className="truncate pt-0.5 text-[11px] text-muted-foreground leading-tight">
          {model.providerLabel}
        </div>
      </div>
      {selected && <CheckCircle2 size={12} className={cn('shrink-0', tone.text)} />}
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
  const tone = getRoleTone(role)

  useEffect(() => {
    if (!isDropdownOpen) return
    const handler = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setIsDropdownOpen(false)
        setSearchQuery('')
      }
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [isDropdownOpen])

  useEffect(() => {
    if (isDropdownOpen && inputRef.current) {
      inputRef.current.focus()
    }
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
    <div className="space-y-3">
      {/* Recent quick-picks */}
      {recentModels.length > 0 && (
        <div>
          <div className="mb-2 text-[10px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">
            {t('pickers.recent')}
          </div>
          <div className={cn('grid gap-2', 'grid-cols-2')}>
            {recentModels.map((model) => (
              <QuickPickCell
                key={getQualifiedModel(model)}
                model={model}
                selected={getQualifiedModel(model) === value}
                tone={tone}
                onSelect={handleSelect}
              />
            ))}
          </div>
        </div>
      )}

      {/* Dropdown search selector */}
      <div className="relative" ref={dropdownRef}>
        <button
          type="button"
          onClick={() => setIsDropdownOpen(!isDropdownOpen)}
          aria-expanded={isDropdownOpen}
          className={cn(
            'flex w-full items-center gap-3 rounded-xl border px-3.5 py-3 text-left transition-all hover:border-foreground/12 hover:bg-muted/50 cursor-pointer',
            selectedModel && !isRecentSelection ? tone.border : 'border-border/60'
          )}
        >
          {selectedModel && !isRecentSelection ? (
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-semibold leading-tight text-foreground">
                {selectedModel.displayName}
              </div>
              <div className="truncate pt-0.5 text-xs text-muted-foreground">
                {selectedModel.providerLabel}
              </div>
            </div>
          ) : (
            <div className="flex min-w-0 flex-1 items-center gap-2">
              <Search size={14} className="shrink-0 text-muted-foreground" />
              <span className="text-sm text-muted-foreground">
                {recentModels.length > 0 ? t('pickers.allModels') : t('pickers.selectModel')}
              </span>
            </div>
          )}
          <ChevronDown
            size={13}
            className={cn(
              'shrink-0 text-muted-foreground transition-transform',
              isDropdownOpen && 'rotate-180'
            )}
          />
        </button>

        {isDropdownOpen && (
          <div
            className={cn(
              'absolute left-0 right-0 z-50 overflow-hidden rounded-xl border border-border/60 bg-popover shadow-2xl backdrop-blur-lg',
              dropUp ? 'bottom-full mb-1' : 'top-full mt-1'
            )}
          >
            <div className="p-2">
              <div className="relative">
                <Search
                  size={13}
                  className="absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground"
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
                  className="w-full rounded-lg border border-border/60 bg-muted py-2 pl-8 pr-3 text-sm text-foreground placeholder:text-muted-foreground focus:border-foreground/20 focus:outline-none"
                />
              </div>
            </div>
            <div className="max-h-60 overflow-y-auto px-2 pb-2 scrollbar-thin">
              {filteredDropdownModels.length === 0 ? (
                <div className="py-5 text-center text-sm text-muted-foreground">
                  {t('pickers.noModels')}
                </div>
              ) : (
                filteredDropdownModels.map((model) => {
                  const selected = getQualifiedModel(model) === value
                  const showSourceProvider =
                    model.sourceProviderLabel && model.sourceProviderLabel !== model.providerLabel

                  return (
                    <button
                      key={getQualifiedModel(model)}
                      type="button"
                      onClick={() => handleSelect(model)}
                      className={cn(
                        'flex w-full items-start gap-3 rounded-xl px-3 py-2.5 text-left transition-all cursor-pointer',
                        selected ? tone.bgSelected : 'hover:bg-muted/40'
                      )}
                    >
                      <div
                        className={cn(
                          'inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md border',
                          model.available ? tone.border : 'border-border',
                          model.available ? tone.background : 'bg-muted'
                        )}
                      >
                        {model.supportsPairExecution ? (
                          tone.iconSm
                        ) : (
                          <CircleAlert size={10} className="text-amber-600 dark:text-amber-400" />
                        )}
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-sm font-semibold leading-tight text-foreground">
                          {model.displayName}
                        </div>
                        <div className="mt-1 flex flex-wrap items-center gap-x-1.5 gap-y-1 text-[11px] text-muted-foreground">
                          {showSourceProvider && (
                            <>
                              <span className="font-medium text-blue-600 dark:text-blue-400">
                                {model.sourceProviderLabel}
                              </span>
                              <span>via</span>
                            </>
                          )}
                          <span className="font-medium text-foreground/80">
                            {model.providerLabel}
                          </span>
                          {model.planLabel && model.planLabel !== 'BYOK' && (
                            <span className="inline-flex items-center rounded-full bg-emerald-500/15 px-1.5 py-0.5 text-[9px] font-semibold uppercase text-emerald-600 dark:text-emerald-400">
                              {model.planLabel}
                            </span>
                          )}
                        </div>
                      </div>
                      {selected && <CheckCircle2 size={11} className={cn('shrink-0', tone.text)} />}
                    </button>
                  )
                })
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
  )

  if (variant === 'card') {
    return (
      <div
        className={cn(
          'flex flex-col rounded-xl border p-3.5 ring-1 ring-border/30',
          tone.border,
          'bg-card shadow-sm'
        )}
      >
        {/* Card header */}
        <div className="mb-2.5 flex items-center gap-2.5">
          <div
            className={cn(
              'inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border',
              tone.border,
              tone.background
            )}
          >
            {tone.icon}
          </div>
          <div>
            <div className={cn('text-sm font-semibold', tone.text)}>
              {role === 'mentor' ? t('pickers.mentorLabel') : t('pickers.executorLabel')}
            </div>
            <div className="text-xs text-muted-foreground">
              {role === 'mentor' ? t('pickers.mentorDesc') : t('pickers.executorDesc')}
            </div>
          </div>
        </div>
        {pickerContent}
      </div>
    )
  }

  return pickerContent
}
