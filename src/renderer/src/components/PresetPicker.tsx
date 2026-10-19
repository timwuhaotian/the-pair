import React, { useState, useRef, useCallback, useLayoutEffect, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { Bug, RefreshCw, Sparkles, Shield, HelpCircle, FlaskConical } from 'lucide-react'
import { cn } from '../lib/utils'
import { Z, errorCallout, hoverTint, popoverSurface } from '../lib/ui'
import { GlassButton } from './ui/GlassButton'
import { IconButton } from './ui/IconButton'
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
  children: React.ReactNode
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
      <div
        ref={triggerRef}
        className="inline-flex"
        data-preset-popover
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
      >
        {children}
      </div>
      {open &&
        createPortal(
          <div
            ref={popoverRef}
            className={cn('fixed w-64 p-3 text-[12px]', Z.popover, popoverSurface)}
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
                      className="inline-flex items-center rounded-sm border border-border bg-background px-1 py-px text-[10px] text-foreground/85"
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
  const { t } = useTranslation()
  const c = getTone(preset.id)

  return (
    <div
      className={cn(
        'flex min-h-8 items-center gap-1 rounded-md border pr-1 text-[12px] transition-colors',
        selected ? cn(c.border, c.bg) : cn('border-border bg-background', hoverTint)
      )}
    >
      <button
        type="button"
        className="flex min-w-0 flex-1 cursor-pointer items-baseline gap-2 self-stretch py-1.5 pl-2 text-left"
        onClick={onSelect}
        aria-pressed={selected}
        data-testid={`preset-card-${preset.id}`}
      >
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
      </button>
      <PresetPopover preset={preset}>
        <IconButton aria-label={t('pickers.presetInfo', { name: preset.name })} className="h-5 w-5">
          <HelpCircle size={11} />
        </IconButton>
      </PresetPopover>
    </div>
  )
}

function SkeletonCard(): React.ReactNode {
  return (
    <div
      className="h-8 rounded-md border border-border bg-background/40 animate-pulse"
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
      <div role="alert" className={cn('flex flex-col items-start gap-1.5', errorCallout)}>
        <span>✗ {t('pickers.noPresets')}</span>
        <p className="text-[11px] text-muted-foreground [overflow-wrap:anywhere]">
          {error ? error : t('pickers.presetsError')}
        </p>
        {onRetry && (
          <GlassButton
            variant="secondary"
            size="sm"
            onClick={onRetry}
            icon={<RefreshCw size={12} />}
          >
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
