import React, { useEffect, useState, useRef, useMemo } from 'react'
import { ChevronDown, GitBranch, RefreshCw, Search, X } from 'lucide-react'
import Fuse from 'fuse.js'
import { tauriApi } from '../lib/tauri-api'
import type { RepoState } from '../types'
import { cn } from '../lib/utils'

interface BranchPickerProps {
  directory: string
  value?: string
  onChange: (branch: string | undefined) => void
  className?: string
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
  className
}: BranchPickerProps): React.ReactNode {
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
      <div
        className={cn(
          'flex items-baseline gap-2 font-mono text-[11px] text-muted-foreground',
          className
        )}
      >
        <RefreshCw className="h-3 w-3 animate-spin translate-y-px" />
        <span>· checking repo…</span>
      </div>
    )
  }

  if (loadError) {
    return (
      <div
        className={cn(
          'flex flex-col gap-0.5 font-mono text-[11px] text-muted-foreground',
          className
        )}
      >
        <div className="flex items-baseline gap-2">
          <GitBranch className="h-3 w-3 translate-y-px" />
          <span>! repo state unreadable</span>
        </div>
        {errorMessage && (
          <div className="text-[10px] state-error ml-5 [overflow-wrap:anywhere]">
            {errorMessage}
          </div>
        )}
      </div>
    )
  }

  if (!repoState?.isGitRepo) {
    return (
      <div
        className={cn(
          'flex items-baseline gap-2 font-mono text-[11px] text-muted-foreground',
          className
        )}
      >
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

  return (
    <div ref={containerRef} className={cn('relative font-mono', className)}>
      <button
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        className={cn(
          'w-full flex items-center justify-between gap-2 px-2 py-1.5 text-[12px] rounded-sm border transition-colors cursor-pointer',
          'bg-background',
          value ? 'border-role-mentor' : 'border-border hover:border-foreground/40'
        )}
      >
        <div className="flex items-baseline gap-2 min-w-0">
          <GitBranch className="h-3 w-3 translate-y-px text-muted-foreground shrink-0" />
          {selectedBranch ? (
            <span className="role-mentor truncate">{selectedBranch.name}</span>
          ) : (
            <span className="text-muted-foreground">select branch (optional)</span>
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
        <div className="absolute z-50 mt-1 w-full max-h-64 overflow-auto rounded-sm border border-border bg-popover">
          <div className="sticky top-0 z-10 border-b border-border bg-popover px-2 py-1.5">
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
                  if (e.key === 'Escape') {
                    setIsOpen(false)
                    setSearch('')
                  }
                  if (e.key === 'Enter') {
                    const first = filteredLocal[0] ?? filteredRemote[0]
                    if (first) handleSelect(first.name)
                  }
                }}
                placeholder="filter branches…"
                className="w-full pl-7 pr-6 py-1 text-[11px] rounded-sm bg-background border border-border focus:border-foreground/40 focus:outline-none text-foreground placeholder:text-muted-foreground-faint"
              />
              {search && (
                <button
                  type="button"
                  onClick={() => setSearch('')}
                  className="absolute right-1.5 top-1/2 -translate-y-1/2 text-muted-foreground-faint hover:text-foreground cursor-pointer"
                >
                  <X size={10} />
                </button>
              )}
            </div>
          </div>

          <button
            type="button"
            onClick={handleClear}
            className={cn(
              'w-full text-left px-3 py-1.5 text-[11px] hover:bg-foreground/[0.05] cursor-pointer text-muted-foreground',
              !value && 'bg-foreground/[0.05] text-foreground/90'
            )}
          >
            — no branch (work in directory)
          </button>

          {repoState.isDirty && (
            <div className="px-3 py-1.5 text-[10px] state-running bg-state-running/8 border-t border-border">
              ! uncommitted changes — commit or stash before selecting a branch
            </div>
          )}

          {filteredLocal.length > 0 && (
            <div className="border-t border-border">
              <div className="px-3 py-1 text-[10px] uppercase tracking-[0.16em] text-muted-foreground bg-background/40">
                ── local ──
              </div>
              {filteredLocal.map((branch) => {
                const isCurrent = repoState.currentBranch === branch.name
                return (
                  <button
                    key={branch.name}
                    type="button"
                    disabled={repoState.isDirty && !isCurrent}
                    onClick={() => handleSelect(branch.name)}
                    className={cn(
                      'w-full flex items-baseline justify-between gap-2 px-3 py-1 text-[11px] hover:bg-foreground/[0.05] cursor-pointer',
                      value === branch.name && 'bg-role-mentor',
                      repoState.isDirty && !isCurrent && 'opacity-40 cursor-not-allowed'
                    )}
                  >
                    <div className="flex items-baseline gap-2 min-w-0">
                      {isCurrent && (
                        <span className="text-[9px] uppercase tracking-[0.14em] state-done border border-state-done px-1">
                          current
                        </span>
                      )}
                      <span className={cn('text-foreground/90 truncate', isCurrent && 'font-bold')}>
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
            <div className="border-t border-border">
              <div className="px-3 py-1 text-[10px] uppercase tracking-[0.16em] text-muted-foreground bg-background/40">
                ── remote ──
              </div>
              {filteredRemote.map((branch) => (
                <button
                  key={branch.name}
                  type="button"
                  disabled={repoState.isDirty}
                  onClick={() => handleSelect(branch.name)}
                  className={cn(
                    'w-full flex items-baseline justify-between gap-2 px-3 py-1 text-[11px] hover:bg-foreground/[0.05] cursor-pointer',
                    value === branch.name && 'bg-role-mentor',
                    repoState.isDirty && 'opacity-40 cursor-not-allowed'
                  )}
                >
                  <div className="flex flex-col items-start min-w-0">
                    <span className="text-foreground/90 truncate">{branch.name}</span>
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
              ))}
            </div>
          )}

          {search && filteredLocal.length === 0 && filteredRemote.length === 0 && (
            <div className="px-3 py-3 text-[11px] text-muted-foreground-faint text-center">
              — no branches match &ldquo;{search}&rdquo; —
            </div>
          )}
        </div>
      )}
    </div>
  )
}
