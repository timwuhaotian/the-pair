import React, { useEffect, useState, useRef, useMemo, useLayoutEffect, useCallback } from 'react'
import { createPortal } from 'react-dom'
import { ChevronDown, GitBranch, Loader2, Search, X } from 'lucide-react'
import Fuse from 'fuse.js'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import { tauriApi } from '../lib/tauri-api'
import type { RepoState } from '../types'
import { cn } from '../lib/utils'
import {
  Z,
  emptyHint,
  errorCallout,
  groupHeader,
  menuItem,
  popoverSurface,
  searchInput,
  warningCallout
} from '../lib/ui'
import { Chip } from './ui/Chip'
import { IconButton } from './ui/IconButton'
import { isImeKeyEvent } from './keyboard'

interface BranchPickerProps {
  directory: string
  value?: string
  onChange: (branch: string | undefined) => void
  className?: string
  /** `branch`: the picked branch is where the pair works (default). `base`:
   *  the picked branch is what a worktree starts from — none means current. */
  variant?: 'branch' | 'base'
}

function formatDate(timestamp: number, t: TFunction): string {
  const date = new Date(timestamp * 1000)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24))
  if (diffDays === 0) return t('pickers.today')
  if (diffDays === 1) return t('pickers.yesterday')
  if (diffDays < 7) return t('pickers.daysAgo', { count: diffDays })
  if (diffDays < 30) return t('pickers.weeksAgo', { count: Math.floor(diffDays / 7) })
  return date.toLocaleDateString()
}

export function BranchPicker({
  directory,
  value,
  onChange,
  className,
  variant = 'branch'
}: BranchPickerProps): React.ReactNode {
  const { t } = useTranslation()
  const isBaseVariant = variant === 'base'
  const [repoState, setRepoState] = useState<RepoState | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const [isOpen, setIsOpen] = useState(false)
  const [loadError, setLoadError] = useState(false)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [search, setSearch] = useState('')

  const loadingRef = useRef(false)
  const containerRef = useRef<HTMLDivElement>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const searchInputRef = useRef<HTMLInputElement>(null)
  const [menuPos, setMenuPos] = useState({ top: 0, left: 0, width: 0 })

  const localBranches = useMemo(
    () => repoState?.branches.filter((b) => b.isLocal) ?? [],
    [repoState?.branches]
  )
  const localBranchNames = useMemo(() => new Set(localBranches.map((b) => b.name)), [localBranches])
  const remoteBranches = useMemo(
    () =>
      (repoState?.branches.filter((b) => b.isRemote) ?? []).filter((b) => {
        const localName = b.name.includes('/') ? b.name.split('/').slice(1).join('/') : b.name
        return !localBranchNames.has(localName)
      }),
    [repoState?.branches, localBranchNames]
  )
  const fuseOptions = useMemo(() => ({ threshold: 0.4, keys: ['name'] }), [])
  const filteredLocal = useMemo(() => {
    if (!search.trim()) return localBranches
    return new Fuse(localBranches, fuseOptions).search(search).map((r) => r.item)
  }, [localBranches, search, fuseOptions])
  const filteredRemote = useMemo(() => {
    if (!search.trim()) return remoteBranches
    return new Fuse(remoteBranches, fuseOptions).search(search).map((r) => r.item)
  }, [remoteBranches, search, fuseOptions])

  useEffect(() => {
    if (!directory) return
    const controller = new AbortController()
    loadingRef.current = true
    setRepoState(null)
    const fetchState = async (): Promise<void> => {
      try {
        setIsLoading(true)
        setLoadError(false)
        const state = await tauriApi.repo.checkState(directory)
        if (!controller.signal.aborted) setRepoState(state)
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        if (!controller.signal.aborted) {
          setRepoState(null)
          setLoadError(true)
          setErrorMessage(msg)
        }
      } finally {
        if (!controller.signal.aborted) {
          setIsLoading(false)
          loadingRef.current = false
        }
      }
    }
    void fetchState()
    return () => controller.abort()
  }, [directory])

  // The menu lives in a portal so modal scroll containers never clip it: it
  // is anchored to the trigger and flips up when the viewport runs out.
  const placeMenu = useCallback(() => {
    const trigger = triggerRef.current
    const menu = menuRef.current
    if (!trigger || !menu) return
    const rect = trigger.getBoundingClientRect()
    const gap = 4
    const height = menu.offsetHeight
    const fitsBelow = rect.bottom + gap + height <= window.innerHeight - 8
    const fitsAbove = rect.top - gap - height >= 8
    const openUp = !fitsBelow && (fitsAbove || rect.top > window.innerHeight - rect.bottom)
    const width = rect.width
    setMenuPos({
      top: openUp ? Math.max(8, rect.top - gap - height) : rect.bottom + gap,
      left: Math.max(8, Math.min(rect.left, window.innerWidth - width - 8)),
      width
    })
  }, [])

  useLayoutEffect(() => {
    if (isOpen) placeMenu()
  }, [isOpen, search, filteredLocal.length, filteredRemote.length, placeMenu])

  useEffect(() => {
    if (!isOpen) return
    const handleClickOutside = (e: MouseEvent): void => {
      const target = e.target as Node
      if (menuRef.current?.contains(target) || containerRef.current?.contains(target)) return
      setIsOpen(false)
    }
    const handleViewport = (): void => placeMenu()
    document.addEventListener('mousedown', handleClickOutside)
    window.addEventListener('resize', handleViewport)
    window.addEventListener('scroll', handleViewport, true)
    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
      window.removeEventListener('resize', handleViewport)
      window.removeEventListener('scroll', handleViewport, true)
    }
  }, [isOpen, placeMenu])

  useEffect(() => {
    if (isOpen) {
      setSearch('')
      requestAnimationFrame(() => searchInputRef.current?.focus())
    }
  }, [isOpen])

  if (!directory) return null

  if (isLoading) {
    return (
      <div className={cn('flex items-center gap-2 text-[12px] text-muted-foreground', className)}>
        <Loader2 size={12} className="animate-spin text-muted-foreground" />
        <span>{t('pickers.checkingRepo')}</span>
      </div>
    )
  }

  if (loadError) {
    return (
      <div role="alert" className={cn('flex flex-col gap-0.5', errorCallout, className)}>
        <span>✗ {t('pickers.repoError')}</span>
        {errorMessage && (
          <span className="text-[11px] text-muted-foreground [overflow-wrap:anywhere]">
            {t('pickers.gitError', { message: errorMessage })}
          </span>
        )}
      </div>
    )
  }

  if (!repoState?.isGitRepo) {
    return (
      <div className={cn('flex items-baseline gap-2 text-[12px] text-muted-foreground', className)}>
        <GitBranch className="h-3 w-3 translate-y-px" />
        <span>{t('pickers.notGitRepo')}</span>
      </div>
    )
  }

  const selectedBranch = repoState.branches.find((b) => b.name === value)

  const handleSelect = (branchName: string): void => {
    if (value === branchName) onChange(undefined)
    else onChange(branchName)
    setIsOpen(false)
  }

  const handleClear = (): void => {
    onChange(undefined)
    setIsOpen(false)
  }

  // Mirrors the `disabled` state of the rows below (dirty tree → only the current branch).
  const isBranchSelectable = (branch: { name: string; isLocal: boolean }): boolean =>
    !repoState.isDirty || (branch.isLocal && repoState.currentBranch === branch.name)

  // Escape inside the open dropdown closes only the dropdown, never the enclosing modal.
  const handleContainerKeyDown = (e: React.KeyboardEvent<HTMLDivElement>): void => {
    if (!isOpen || e.key !== 'Escape' || isImeKeyEvent(e)) return
    e.preventDefault()
    e.stopPropagation()
    setIsOpen(false)
    setSearch('')
  }

  return (
    <div
      ref={containerRef}
      className={cn('relative', className)}
      onKeyDown={handleContainerKeyDown}
    >
      <button
        ref={triggerRef}
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        className={cn(
          'flex w-full cursor-pointer items-center justify-between gap-2 rounded-sm border bg-background px-2 py-1.5 text-[12px] transition-colors',
          value ? 'border-role-mentor' : 'border-border hover:border-foreground/40'
        )}
      >
        <div className="flex items-baseline gap-2 min-w-0">
          <GitBranch className="h-3 w-3 translate-y-px text-muted-foreground shrink-0" />
          {selectedBranch ? (
            <span className="role-mentor truncate [overflow-wrap:anywhere]">
              {selectedBranch.name}
            </span>
          ) : (
            <span className="text-muted-foreground">
              {isBaseVariant ? t('pickers.baseBranch') : t('pickers.selectBranch')}
            </span>
          )}
        </div>
        <ChevronDown
          className={cn(
            'h-3 w-3 text-muted-foreground-faint transition-transform shrink-0',
            isOpen && 'rotate-180'
          )}
        />
      </button>

      {isOpen &&
        createPortal(
          <div
            ref={menuRef}
            onKeyDown={handleContainerKeyDown}
            className={cn('fixed max-h-64 overflow-auto', Z.popover, popoverSurface)}
            style={{ top: menuPos.top, left: menuPos.left, width: menuPos.width }}
          >
            <div className="sticky top-0 z-10 border-b border-border bg-popover p-1.5">
              <div className="relative">
                <Search
                  size={11}
                  className="absolute left-2 top-1/2 -translate-y-1/2 text-muted-foreground-faint pointer-events-none"
                />
                <input
                  ref={searchInputRef}
                  type="text"
                  value={search}
                  aria-label={t('pickers.filterBranches')}
                  onChange={(e) => setSearch(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key !== 'Enter') return
                    // Enter picks the top match; it must never submit the enclosing form
                    // (nor pick anything when it only confirms an IME candidate).
                    e.preventDefault()
                    e.stopPropagation()
                    if (isImeKeyEvent(e)) return
                    const first = [...filteredLocal, ...filteredRemote].find((branch) =>
                      isBranchSelectable(branch)
                    )
                    if (first) handleSelect(first.name)
                  }}
                  placeholder={t('pickers.filterBranches')}
                  className={cn(searchInput, 'pr-7')}
                />
                {search && (
                  <IconButton
                    aria-label={t('pickers.clearFilter')}
                    onClick={() => setSearch('')}
                    className="absolute right-0.5 top-1/2 h-6 w-6 -translate-y-1/2"
                  >
                    <X size={10} />
                  </IconButton>
                )}
              </div>
            </div>

            <div role="listbox" className="p-1">
              <button
                type="button"
                role="option"
                aria-selected={!value}
                onClick={handleClear}
                className={cn(menuItem(!value), value && 'text-muted-foreground')}
              >
                — {isBaseVariant ? t('pickers.fromCurrentBranch') : t('pickers.noBranch')}
              </button>

              {repoState.isDirty && (
                <div className={cn('my-1', warningCallout)} title={t('pickers.uncommittedChanges')}>
                  {t('pickers.dirtyWarning', {
                    hint: isBaseVariant ? t('pickers.dirtyBaseHint') : t('pickers.dirtyBranchHint')
                  })}
                </div>
              )}

              {filteredLocal.length > 0 && (
                <div
                  role="group"
                  aria-label={t('pickers.localBranches')}
                  className="mt-1 border-t border-border pt-1"
                >
                  <div className={groupHeader}>── {t('pickers.localBranches')} ──</div>
                  {filteredLocal.map((branch) => {
                    const isCurrent = repoState.currentBranch === branch.name
                    const disabled = repoState.isDirty && !isCurrent
                    const selected = value === branch.name
                    return (
                      <button
                        key={branch.name}
                        type="button"
                        role="option"
                        aria-selected={selected}
                        disabled={disabled}
                        onClick={() => handleSelect(branch.name)}
                        className={cn(
                          menuItem(selected),
                          'items-baseline justify-between',
                          disabled && 'cursor-not-allowed opacity-40 hover:bg-transparent'
                        )}
                      >
                        <div className="flex items-baseline gap-2 min-w-0">
                          {isCurrent && (
                            <Chip tone="done" outline>
                              {t('pickers.current')}
                            </Chip>
                          )}
                          <span
                            className={cn(
                              'truncate [overflow-wrap:anywhere]',
                              selected ? 'role-mentor' : 'text-foreground/90',
                              isCurrent && 'font-medium'
                            )}
                          >
                            {branch.name}
                          </span>
                        </div>
                        {branch.lastCommitMessage && (
                          <span className="text-[10px] text-muted-foreground-faint truncate max-w-[18ch]">
                            {branch.lastCommitMessage}
                          </span>
                        )}
                      </button>
                    )
                  })}
                </div>
              )}

              {filteredRemote.length > 0 && (
                <div
                  role="group"
                  aria-label={t('pickers.remoteBranches')}
                  className="mt-1 border-t border-border pt-1"
                >
                  <div className={groupHeader}>── {t('pickers.remoteBranches')} ──</div>
                  {filteredRemote.map((branch) => {
                    const selected = value === branch.name
                    return (
                      <button
                        key={branch.name}
                        type="button"
                        role="option"
                        aria-selected={selected}
                        disabled={repoState.isDirty}
                        onClick={() => handleSelect(branch.name)}
                        className={cn(
                          menuItem(selected),
                          'items-baseline justify-between',
                          repoState.isDirty && 'cursor-not-allowed opacity-40 hover:bg-transparent'
                        )}
                      >
                        <div className="flex flex-col items-start min-w-0">
                          <span
                            className={cn(
                              'truncate [overflow-wrap:anywhere]',
                              selected ? 'role-mentor' : 'text-foreground/90'
                            )}
                          >
                            {branch.name}
                          </span>
                          {!branch.isCheckedOutLocally && (
                            <span className="text-[10px] role-mentor">
                              → {t('pickers.trackingBranch')}
                            </span>
                          )}
                        </div>
                        {branch.lastCommitDate && (
                          <span className="text-[10px] text-muted-foreground-faint">
                            {formatDate(branch.lastCommitDate, t)}
                          </span>
                        )}
                      </button>
                    )
                  })}
                </div>
              )}

              {search && filteredLocal.length === 0 && filteredRemote.length === 0 && (
                <div className={emptyHint}>— {t('pickers.noMatch', { query: search })} —</div>
              )}
            </div>
          </div>,
          document.body
        )}
    </div>
  )
}
