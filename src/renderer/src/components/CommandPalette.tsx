import React, { useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createPortal } from 'react-dom'
import Fuse from 'fuse.js'
import { CornerDownLeft } from 'lucide-react'
import { cn } from '../lib/utils'
import { activeTint, emptyHint, hoverTint, kbd, sectionLabel, Z } from '../lib/ui'
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

  if (!isOpen) return null

  const scrollActiveIntoView = (): void => {
    requestAnimationFrame(() => {
      listRef.current?.querySelector<HTMLElement>('[data-active="true"]')?.scrollIntoView({
        block: 'nearest'
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
    } else if (e.key === 'Escape') {
      e.preventDefault()
      onClose()
    }
  }

  let lastGroup: string | null = null

  return createPortal(
    <div
      className={cn('fixed inset-0 flex items-start justify-center px-4 pt-[14vh]', Z.palette)}
      role="presentation"
    >
      <div aria-hidden className="absolute inset-0 bg-background/80" onMouseDown={onClose} />
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t('palette.title')}
        data-testid="command-palette"
        onKeyDown={handleKeyDown}
        className="shell-card relative w-full max-w-[560px] overflow-hidden bg-popover"
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
            data-testid="command-palette-input"
            autoComplete="off"
            spellCheck={false}
            autoFocus
            className="h-12 w-full bg-transparent text-[14px] text-foreground outline-none placeholder:text-muted-foreground-faint"
          />
          <kbd className={cn(kbd, 'shrink-0')}>esc</kbd>
        </div>
        <div ref={listRef} className="max-h-[320px] overflow-y-auto p-1.5 scrollbar-thin">
          {results.length === 0 ? (
            <p className={emptyHint}>{t('palette.noResults')}</p>
          ) : (
            results.map((item, index) => {
              const groupLabel = t(item.action.groupKey)
              const showGroup = groupLabel !== lastGroup
              lastGroup = groupLabel
              const isActive = index === clampedIndex
              return (
                <React.Fragment key={item.action.id}>
                  {showGroup && (
                    <div className={cn(sectionLabel, 'px-2.5 pb-0.5 pt-2 first:pt-0.5')}>
                      {groupLabel}
                    </div>
                  )}
                  <button
                    type="button"
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
                </React.Fragment>
              )
            })
          )}
        </div>
      </div>
    </div>,
    document.body
  )
}
