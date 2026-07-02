import React, { useState, useRef, useCallback, useLayoutEffect, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import {
  Bug,
  RefreshCw,
  Sparkles,
  Shield,
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
  Bug: <Bug size={11} />,
  RefreshCw: <RefreshCw size={11} />,
  Sparkles: <Sparkles size={11} />,
  Shield: <Shield size={11} />,
  FlaskConical: <FlaskConical size={11} />
}

const presetTones: Record<string, { fg: string; bg: string; border: string }> = {
  'bug-fix': { fg: 'state-error', bg: 'bg-state-error', border: 'border-state-error' },
  refactor: { fg: 'role-mentor', bg: 'bg-role-mentor', border: 'border-role-mentor' },
  feature: { fg: 'role-executor', bg: 'bg-role-executor', border: 'border-role-executor' },
  hardening: { fg: 'state-running', bg: 'bg-state-running', border: 'border-state-running' },
  'dev-smoke-test': { fg: 'state-done', bg: 'bg-state-done', border: 'border-state-done' }
}

function getTone(id: string): { fg: string; bg: string; border: string } {
  return presetTones[id] ?? presetTones['feature']
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
    const popoverWidth = 256
    const gap = 8

    let top: number
    let left = triggerRect.left + triggerRect.width / 2 - popoverWidth / 2
    left = Math.max(8, Math.min(left, window.innerWidth - popoverWidth - 8))

    if (popoverRef.current) {
      top = triggerRect.top - popoverRef.current.offsetHeight - gap
    } else {
      top = triggerRect.top - 180 - gap
    }
    if (top < 8) top = triggerRect.bottom + gap
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
            className="fixed z-[9999] w-64 rounded-sm border border-border bg-popover p-3 font-mono text-[11px]"
            style={{ top: position.top, left: position.left }}
            onMouseEnter={show}
            onFocus={show}
            onMouseLeave={hide}
            onBlur={hide}
          >
            <div className="mb-2 text-muted-foreground leading-relaxed">{preset.description}</div>
            <div className="space-y-1">
              {preset.recommendedSkills.length > 0 && (
                <div className="flex flex-wrap gap-1">
                  {preset.recommendedSkills.map((skill) => (
                    <span
                      key={skill}
                      className="inline-flex items-center border border-border bg-background px-1 py-px text-[10px] text-foreground/85"
                    >
                      {skill}
                    </span>
                  ))}
                </div>
              )}
              {preset.pauseOnIteration && (
                <div className="text-muted-foreground">
                  <span className="text-foreground/85">· auto-pause</span> @ iter{' '}
                  {preset.pauseOnIteration}
                </div>
              )}
              {preset.autoAttachGitBaseline && (
                <div className="text-muted-foreground">
                  <span className="text-foreground/85">· git baseline</span> created on pair start
                </div>
              )}
            </div>
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
  const c = getTone(preset.id)

  return (
    <button
      type="button"
      className={cn(
        'relative border px-2 py-1.5 rounded-sm transition-colors cursor-pointer font-mono text-[11px] text-left w-full',
        selected
          ? cn(c.border, c.bg)
          : 'border-border bg-background hover:border-foreground/40 hover:bg-foreground/[0.04]'
      )}
      onClick={onSelect}
      aria-pressed={selected}
      data-testid={`preset-card-${preset.id}`}
    >
      <div className="flex items-baseline gap-2">
        <span aria-hidden className={cn('shrink-0 translate-y-px', c.fg)}>
          {iconMap[preset.icon] || <Sparkles size={11} />}
        </span>
        <span className={cn('min-w-0 flex-1 truncate', selected ? c.fg : 'text-foreground/90')}>
          {preset.name}
        </span>
        {selected && (
          <span aria-hidden className={c.fg}>
            ✓
          </span>
        )}
        <PresetPopover preset={preset}>
          {
            ((show: () => void, hide: () => void) => (
              <button
                type="button"
                onClick={(e) => e.stopPropagation()}
                onMouseEnter={show}
                onMouseLeave={hide}
                className="shrink-0 text-muted-foreground-faint hover:text-foreground/85 transition-colors"
                aria-label={`info about ${preset.name}`}
              >
                <HelpCircle size={11} />
              </button>
            )) as unknown as React.ReactNode
          }
        </PresetPopover>
      </div>
    </button>
  )
}

function SkeletonCard(): React.ReactNode {
  return (
    <div
      className="rounded-sm border border-border bg-background/40 px-2 py-1.5 animate-pulse h-7"
      data-testid="preset-picker-skeleton"
    />
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
      <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-4">
        <SkeletonCard />
        <SkeletonCard />
        <SkeletonCard />
        <SkeletonCard />
      </div>
    )
  }

  if (!loading && presets.length === 0) {
    return (
      <div className="flex flex-col items-start gap-2 border border-dashed border-border bg-background/40 px-3 py-4 font-mono text-[11px]">
        <div className="flex items-baseline gap-2">
          <AlertCircle size={11} className="state-running translate-y-px" />
          <span className="text-foreground/85">{t('pickers.noPresets')}</span>
        </div>
        <p className="text-[10px] text-muted-foreground [overflow-wrap:anywhere]">
          {error ? error : t('pickers.presetsError')}
        </p>
        {onRetry && (
          <GlassButton variant="secondary" size="sm" onClick={onRetry}>
            <RefreshCw size={10} />
            {t('common.retry')}
          </GlassButton>
        )}
      </div>
    )
  }

  return (
    <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-4">
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
