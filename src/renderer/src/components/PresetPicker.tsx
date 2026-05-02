import React, { useState, useRef, useCallback, useLayoutEffect, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import {
  Bug,
  RefreshCw,
  Sparkles,
  Shield,
  Check,
  AlertCircle,
  HelpCircle,
  FlaskConical
} from 'lucide-react'
import { cn } from '../lib/utils'
import { GlassButton } from './ui/GlassButton'
import type { PairPreset } from '../types'

interface PresetPickerProps {
  presets: PairPreset[]
  selectedPresetId: string | null
  onSelect: (preset: PairPreset | null) => void
  loading?: boolean
  onRetry?: () => void
  error?: string | null
}

const iconMap: Record<string, React.ReactNode> = {
  Bug: <Bug size={16} />,
  RefreshCw: <RefreshCw size={16} />,
  Sparkles: <Sparkles size={16} />,
  Shield: <Shield size={16} />,
  FlaskConical: <FlaskConical size={16} />
}

const presetColors: Record<
  string,
  { border: string; background: string; icon: string; glow: string }
> = {
  'bug-fix': {
    border: 'border-red-500/30',
    background: 'bg-red-500/12 dark:bg-red-500/14',
    icon: 'text-red-600 dark:text-red-400',
    glow: 'hover:shadow-red-500/20'
  },
  refactor: {
    border: 'border-blue-500/30',
    background: 'bg-blue-500/12 dark:bg-blue-500/14',
    icon: 'text-blue-600 dark:text-blue-400',
    glow: 'hover:shadow-blue-500/20'
  },
  feature: {
    border: 'border-purple-500/30',
    background: 'bg-purple-500/12 dark:bg-purple-500/14',
    icon: 'text-purple-600 dark:text-purple-400',
    glow: 'hover:shadow-purple-500/20'
  },
  hardening: {
    border: 'border-amber-500/30',
    background: 'bg-amber-500/12 dark:bg-amber-500/14',
    icon: 'text-amber-600 dark:text-amber-400',
    glow: 'hover:shadow-amber-500/20'
  },
  'dev-smoke-test': {
    border: 'border-emerald-500/30',
    background: 'bg-emerald-500/12 dark:bg-emerald-500/14',
    icon: 'text-emerald-600 dark:text-emerald-400',
    glow: 'hover:shadow-emerald-500/20'
  }
}

function PresetPopover({
  preset,
  children
}: {
  preset: PairPreset
  children: React.ReactNode | ((show: () => void, hide: () => void) => React.ReactNode)
}): React.ReactNode {
  const [open, setOpen] = useState(false)
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const triggerRef = useRef<HTMLDivElement>(null)
  const popoverRef = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState({ top: 0, left: 0 })

  const show = useCallback(() => {
    if (timeoutRef.current) clearTimeout(timeoutRef.current)
    setOpen(true)
  }, [])
  const hide = useCallback(() => {
    if (timeoutRef.current) clearTimeout(timeoutRef.current)
    timeoutRef.current = setTimeout(() => setOpen(false), 150)
  }, [])

  useEffect(() => {
    return () => {
      if (timeoutRef.current) clearTimeout(timeoutRef.current)
    }
  }, [])

  useLayoutEffect(() => {
    if (!open || !triggerRef.current) return
    const triggerRect = triggerRef.current.getBoundingClientRect()
    const popoverWidth = 256 // w-64
    const gap = 8

    let top: number
    let left = triggerRect.left + triggerRect.width / 2 - popoverWidth / 2
    left = Math.max(8, Math.min(left, window.innerWidth - popoverWidth - 8))

    if (popoverRef.current) {
      top = triggerRect.top - popoverRef.current.offsetHeight - gap
    } else {
      top = triggerRect.top - 180 - gap
    }

    if (top < 8) {
      top = triggerRect.bottom + gap
    }

    setPosition({ top, left })
  }, [open])

  return (
    <>
      <div ref={triggerRef} className="inline-block" data-preset-popover>
        {typeof children === 'function'
          ? // eslint-disable-next-line react-hooks/refs
            (children as (show: () => void, hide: () => void) => React.ReactNode)(show, hide)
          : children}
      </div>
      {open &&
        createPortal(
          <div
            ref={popoverRef}
            className="fixed z-[9999] w-64 rounded-xl border border-border/80 bg-popover shadow-xl p-3 text-xs"
            style={{ top: position.top, left: position.left }}
            onMouseEnter={show}
            onFocus={show}
            onMouseLeave={hide}
            onBlur={hide}
          >
            <div className="mb-2 text-muted-foreground leading-relaxed">{preset.description}</div>
            <div className="space-y-1.5">
              <div className="flex flex-wrap gap-1">
                {preset.recommendedSkills.map((skill) => (
                  <span
                    key={skill}
                    className="inline-flex items-center rounded-full bg-muted/60 px-2 py-0.5 text-[10px] font-medium text-foreground"
                  >
                    {skill}
                  </span>
                ))}
              </div>

              {preset.pauseOnIteration && (
                <div className="text-muted-foreground">
                  <span className="font-medium text-foreground">Auto-pause checkpoint</span> at
                  iteration {preset.pauseOnIteration}
                </div>
              )}

              {preset.autoAttachGitBaseline && (
                <div className="text-muted-foreground">
                  <span className="font-medium text-foreground">Git baseline</span> created on pair
                  start
                </div>
              )}
            </div>
            <div className="absolute left-1/2 -translate-x-1/2 top-full w-2 h-2 rotate-45 border-b border-r border-border/80 bg-popover" />
          </div>,
          document.body
        )}
    </>
  )
}

function PresetCard({
  preset,
  selected,
  onSelect
}: {
  preset: PairPreset
  selected: boolean
  onSelect: () => void
}): React.ReactNode {
  const colors = presetColors[preset.id] || presetColors['feature']

  return (
    <div
      className={cn(
        'relative rounded-xl border px-3 py-2 transition-all cursor-pointer',
        selected
          ? cn(colors.border, colors.background)
          : 'border-border/60 bg-muted hover:border-foreground/12 hover:bg-muted/80',
        colors.glow
      )}
      onClick={onSelect}
      data-testid={`preset-card-${preset.id}`}
    >
      {selected && (
        <div className="absolute right-1 top-1">
          <div
            className={cn(
              'flex h-4 w-4 items-center justify-center rounded-full',
              colors.background,
              colors.border
            )}
          >
            <Check size={10} className={colors.icon} />
          </div>
        </div>
      )}

      <div className="flex items-center gap-2">
        <div
          className={cn(
            'inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border',
            colors.border,
            colors.background
          )}
        >
          <span className={colors.icon}>{iconMap[preset.icon] || <Sparkles size={16} />}</span>
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1">
            <h3 className="text-xs font-semibold text-foreground truncate">{preset.name}</h3>
            <PresetPopover preset={preset}>
              {
                ((show: () => void, hide: () => void) => (
                  <button
                    type="button"
                    onClick={(e) => e.stopPropagation()}
                    onMouseEnter={show}
                    onMouseLeave={hide}
                    className="shrink-0 rounded text-muted-foreground hover:text-foreground transition-colors p-0.5"
                    aria-label={`Info about ${preset.name}`}
                  >
                    <HelpCircle size={12} />
                  </button>
                )) as unknown as React.ReactNode
              }
            </PresetPopover>
          </div>
        </div>
      </div>
    </div>
  )
}

function SkeletonCard(): React.ReactNode {
  return (
    <div
      className="rounded-xl border border-border/60 bg-muted px-3 py-2 animate-pulse"
      data-testid="preset-picker-skeleton"
    >
      <div className="flex items-center gap-2">
        <div className="h-8 w-8 shrink-0 rounded-lg border border-border/50 bg-muted/60" />
        <div className="h-3.5 w-20 rounded bg-muted/60" />
      </div>
    </div>
  )
}

export function PresetPicker({
  presets,
  selectedPresetId,
  onSelect,
  loading = false,
  onRetry,
  error
}: PresetPickerProps): React.ReactNode {
  const { t } = useTranslation()
  if (loading) {
    return (
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        <SkeletonCard />
        <SkeletonCard />
        <SkeletonCard />
        <SkeletonCard />
      </div>
    )
  }

  if (!loading && presets.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center gap-3 rounded-2xl border border-border/60 bg-muted py-10 text-center">
        <AlertCircle size={20} className="text-muted-foreground" />
        <div className="space-y-1">
          <p className="text-sm font-medium text-foreground">{t('pickers.noPresets')}</p>
          <p className="text-xs text-muted-foreground">
            {error ? error : t('pickers.presetsError')}
          </p>
        </div>
        {onRetry && (
          <GlassButton variant="secondary" size="sm" onClick={onRetry}>
            <RefreshCw size={12} />
            {t('common.retry')}
          </GlassButton>
        )}
      </div>
    )
  }

  return (
    <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
      {presets.map((preset) => (
        <PresetCard
          key={preset.id}
          preset={preset}
          selected={selectedPresetId === preset.id}
          onSelect={() => onSelect(selectedPresetId === preset.id ? null : preset)}
        />
      ))}
    </div>
  )
}
