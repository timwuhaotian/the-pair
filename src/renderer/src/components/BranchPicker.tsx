import React, { useEffect, useState, useRef, useMemo } from 'react'
import { ChevronDown, GitBranch, Loader2, Search, X } from 'lucide-react'
import Fuse from 'fuse.js'
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

function formatDate(timestamp: number): string {
  const date = new Date(timestamp * 1000)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24))
  if (diffDays === 0) return 'today'
  if (diffDays === 1) return 'yesterday'
  if (diffDays < 7) return `${diffDays}d ago`
  if (diffDays < 30) return `${Math.floor(diffDays / 7)}w ago`
  return date.toLocaleDateString()
}

export function BranchPicker({
  directory,
  value,
  onChange,
  className,
  variant = 'branch'
}: BranchPickerProps): React.ReactNode {
  const isBaseVariant = variant === 'base'
  const [repoState, setRepoState] = useState<RepoState | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const [isOpen, setIsOpen] = useState(false)
  const [loadError, setLoadError] = useState(false)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [search, setSearch] = useState('')

  const loadingRef = useRef(false)
  const containerRef = useRef<HTMLDivElement>(null)
  const searchInputRef = useRef<HTMLInputElement>(null)

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

  useEffect(() => {
    if (!isOpen) return
    const handleClickOutside = (e: MouseEvent): void => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) setIsOpen(false)
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [isOpen])

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
        <span>checking repo…</span>
      </div>
    )
  }

  if (loadError) {
    return (
      <div role="alert" className={cn('flex flex-col gap-0.5', errorCallout, className)}>
        <span>✗ repo state unreadable</span>
        {errorMessage && (
          <span className="text-[11px] text-muted-foreground [overflow-wrap:anywhere]">
            {errorMessage}
          </span>
        )}
      </div>
    )
  }

  if (!repoState?.isGitRepo) {
    return (
      <div className={cn('flex items-baseline gap-2 text-[12px] text-muted-foreground', className)}>
        <GitBranch className="h-3 w-3 translate-y-px" />
        <span>· not a git repo — worktrees unavailable</span>
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
            <span className="role-mentor truncate">{selectedBranch.name}</span>
          ) : (
            <span className="text-muted-foreground">
              {isBaseVariant ? 'base branch (defaults to current)' : 'select branch (optional)'}
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

      {isOpen && (
        <div
          className={cn('absolute mt-1 w-full max-h-64 overflow-auto', Z.popover, popoverSurface)}
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
                placeholder="filter branches…"
                className={cn(searchInput, 'pr-7')}
              />
              {search && (
                <IconButton
                  aria-label="clear filter"
                  onClick={() => setSearch('')}
                  className="absolute right-0.5 top-1/2 h-5 w-5 -translate-y-1/2"
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
              — {isBaseVariant ? 'from current branch' : 'no branch (work in directory)'}
            </button>

            {repoState.isDirty && (
              <div className={cn('my-1', warningCallout)}>
                ! uncommitted changes —{' '}
                {isBaseVariant
                  ? 'only the current branch can be the base'
                  : 'commit or stash before selecting a branch'}
              </div>
            )}

            {filteredLocal.length > 0 && (
              <div role="group" aria-label="local" className="mt-1 border-t border-border pt-1">
                <div className={groupHeader}>── local ──</div>
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
                            current
                          </Chip>
                        )}
                        <span
                          className={cn(
                            'truncate',
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
              <div role="group" aria-label="remote" className="mt-1 border-t border-border pt-1">
                <div className={groupHeader}>── remote ──</div>
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
                            'truncate',
                            selected ? 'role-mentor' : 'text-foreground/90'
                          )}
                        >
                          {branch.name}
                        </span>
                        {!branch.isCheckedOutLocally && (
                          <span className="text-[10px] role-mentor">
                            → creates local tracking branch
                          </span>
                        )}
                      </div>
                      {branch.lastCommitDate && (
                        <span className="text-[10px] text-muted-foreground-faint">
                          {formatDate(branch.lastCommitDate)}
                        </span>
                      )}
                    </button>
                  )
                })}
              </div>
            )}

            {search && filteredLocal.length === 0 && filteredRemote.length === 0 && (
              <div className={emptyHint}>— no branches match &ldquo;{search}&rdquo; —</div>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
