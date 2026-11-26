import React, { useEffect, useId, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createPortal } from 'react-dom'
import Fuse from 'fuse.js'
import { CornerDownLeft } from 'lucide-react'
import { useReducedMotion } from 'framer-motion'
import { cn } from '../lib/utils'
import { activeTint, emptyHint, hoverTint, kbd, sectionLabel, Z } from '../lib/ui'
import { shouldCloseOnEscape } from './keyboard'
import type { PaletteAction } from '../lib/paletteActions'

interface CommandPaletteProps {
  isOpen: boolean
  onClose: () => void
  actions: PaletteAction[]
  pairNameById?: (actionId: string) => string | null
}

interface RankedAction {
  action: PaletteAction
  label: string
}

/** A run of consecutive results under one localized group header. */
interface ResultGroup {
  label: string
  items: Array<{ item: RankedAction; index: number }>
}

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'

/**
 * Global command palette (Cmd+K). Filters the provided actions with fuzzy
 * search, groups them under localized headers, and runs the selected one.
 * Arrow keys move, Enter runs, Esc closes.
 */
export function CommandPalette({
  isOpen,
  onClose,
  actions,
  pairNameById
}: CommandPaletteProps): React.ReactNode {
  const { t } = useTranslation()
  const [query, setQuery] = useState('')
  const [activeIndex, setActiveIndex] = useState(0)
  const listRef = useRef<HTMLDivElement>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const reduceMotion = useReducedMotion()
  const listId = useId()
  const optionId = (index: number): string => `${listId}-option-${index}`

  const available = useMemo(
    () => actions.filter((action) => action.available?.() ?? true),
    [actions]
  )

  const labeled: RankedAction[] = useMemo(
    () =>
      available.map((action) => {
        const pairName = pairNameById?.(action.id) ?? null
        const base = t(action.labelKey)
        return { action, label: pairName ? `${base} — ${pairName}` : base }
      }),
    [available, pairNameById, t]
  )

  const fuse = useMemo(
    () => new Fuse(labeled, { keys: ['label'], threshold: 0.4, ignoreLocation: true }),
    [labeled]
  )

  const results: RankedAction[] = useMemo(() => {
    const trimmed = query.trim()
    if (!trimmed) return labeled
    return fuse.search(trimmed).map((r) => r.item)
  }, [fuse, labeled, query])

  // Active index is clamped during render so query/result changes can never
  // leave it pointing past the end — no reset effects needed.
  const clampedIndex = results.length === 0 ? 0 : Math.min(activeIndex, results.length - 1)

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent): void => {
      if (isOpen && shouldCloseOnEscape(e)) onClose()
    }
    document.addEventListener('keydown', handleEscape)
    return () => document.removeEventListener('keydown', handleEscape)
  }, [isOpen, onClose])

  useEffect(() => {
    if (!isOpen) return
    // Focus goes back here on close — whether `isOpen` turns false or the
    // caller unmounts the palette outright.
    const opener = document.activeElement as HTMLElement | null
    const container = containerRef.current
    inputRef.current?.focus()
    const handleTab = (e: KeyboardEvent): void => {
      if (e.key !== 'Tab') return
      const el = containerRef.current
      if (!el) return
      const nodes = el.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)
      if (nodes.length === 0) {
        e.preventDefault()
        el.focus()
        return
      }
      const first = nodes[0]
      const last = nodes[nodes.length - 1]
      const outside = !el.contains(document.activeElement) || document.activeElement === el
      if (e.shiftKey) {
        if (document.activeElement === first || outside) {
          e.preventDefault()
          last.focus()
        }
      } else {
        if (document.activeElement === last || outside) {
          e.preventDefault()
          first.focus()
        }
      }
    }
    document.addEventListener('keydown', handleTab)
    return () => {
      document.removeEventListener('keydown', handleTab)
      if (!opener || !opener.isConnected) return
      const active = document.activeElement
      // Only reclaim focus the palette held (or lost to <body> with its DOM);
      // an action that moved focus on purpose keeps it.
      if (!active || active === document.body || Boolean(container?.contains(active))) {
        opener.focus?.()
      }
    }
  }, [isOpen])

  if (!isOpen) return null

  const scrollActiveIntoView = (): void => {
    requestAnimationFrame(() => {
      listRef.current?.querySelector<HTMLElement>('[data-active="true"]')?.scrollIntoView({
        block: 'nearest',
        behavior: reduceMotion ? 'auto' : 'smooth'
      })
    })
  }

  const runAt = (index: number): void => {
    const target = results[index]
    if (!target) return
    onClose()
    target.action.run()
  }

  const handleQueryChange = (value: string): void => {
    setQuery(value)
    setActiveIndex(0)
  }

  const moveActive = (delta: number): void => {
    setActiveIndex((i) =>
      results.length === 0 ? 0 : Math.min(Math.max(i + delta, 0), results.length - 1)
    )
    scrollActiveIntoView()
  }

  const handleKeyDown = (e: React.KeyboardEvent): void => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      moveActive(1)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      moveActive(-1)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      runAt(clampedIndex)
    }
  }

  const groups: ResultGroup[] = []
  results.forEach((item, index) => {
    const label = t(item.action.groupKey)
    const current = groups[groups.length - 1]
    if (current && current.label === label) current.items.push({ item, index })
    else groups.push({ label, items: [{ item, index }] })
  })
  const hasResults = results.length > 0

  return createPortal(
    <div
      className={cn('fixed inset-0 flex items-start justify-center px-4 pt-[14vh]', Z.palette)}
      role="presentation"
    >
      <div aria-hidden className="absolute inset-0 bg-background/80" onMouseDown={onClose} />
      <div
        ref={containerRef}
        role="dialog"
        aria-modal="true"
        aria-label={t('palette.title')}
        data-testid="command-palette"
        tabIndex={-1}
        onKeyDown={handleKeyDown}
        className="shell-card relative w-full max-w-[560px] overflow-hidden bg-popover outline-none"
      >
        <div className="flex items-center gap-2 border-b border-border px-3">
          <span aria-hidden className="select-none text-muted-foreground">
            ›
          </span>
          <input
            value={query}
            onChange={(e) => handleQueryChange(e.target.value)}
            placeholder={t('palette.placeholder')}
            aria-label={t('palette.placeholder')}
            role="combobox"
            aria-autocomplete="list"
            aria-expanded={hasResults}
            aria-controls={listId}
            aria-activedescendant={hasResults ? optionId(clampedIndex) : undefined}
            data-testid="command-palette-input"
            ref={inputRef}
            autoComplete="off"
            spellCheck={false}
            className="h-12 w-full bg-transparent text-[14px] text-foreground outline-none placeholder:text-muted-foreground-faint"
          />
          <kbd className={cn(kbd, 'shrink-0')}>esc</kbd>
        </div>
        <div
          ref={listRef}
          id={listId}
          role="listbox"
          aria-label={t('palette.results')}
          className="max-h-[320px] overflow-y-auto p-1.5 scrollbar-thin"
        >
          {!hasResults ? (
            <p className={emptyHint}>{t('palette.noResults')}</p>
          ) : (
            groups.map((group, groupIndex) => {
              const headerId = `${listId}-group-${groupIndex}`
              return (
                <div key={headerId} role="group" aria-labelledby={headerId}>
                  <div
                    id={headerId}
                    className={cn(
                      sectionLabel,
                      'px-2.5 pb-0.5',
                      groupIndex === 0 ? 'pt-0.5' : 'pt-2'
                    )}
                  >
                    {group.label}
                  </div>
                  {group.items.map(({ item, index }) => {
                    const isActive = index === clampedIndex
                    return (
                      // Options stay out of the Tab order: focus lives in the
                      // input and the active row is its aria-activedescendant.
                      <button
                        key={item.action.id}
                        id={optionId(index)}
                        type="button"
                        role="option"
                        aria-selected={isActive}
                        tabIndex={-1}
                        data-active={isActive}
                        data-testid={`palette-item-${item.action.id}`}
                        onMouseEnter={() => setActiveIndex(index)}
                        onClick={() => runAt(index)}
                        className={cn(
                          'flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-[13px] transition-colors cursor-pointer',
                          isActive
                            ? cn(activeTint, 'text-foreground')
                            : cn('text-foreground/85', hoverTint)
                        )}
                      >
                        <span className="min-w-0 flex-1 truncate">{item.label}</span>
                        {item.action.shortcut && (
                          <span className="flex shrink-0 items-center gap-0.5">
                            {item.action.shortcut.map((part, i) => (
                              <kbd key={i} className={kbd}>
                                {part}
                              </kbd>
                            ))}
                          </span>
                        )}
                        {isActive && (
                          <CornerDownLeft
                            size={12}
                            aria-hidden
                            className="shrink-0 text-muted-foreground"
                          />
                        )}
                      </button>
                    )
                  })}
                </div>
              )
            })
          )}
        </div>
      </div>
    </div>,
    document.body
  )
}
