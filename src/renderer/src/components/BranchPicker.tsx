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

  if (diffDays === 0) return 'Today'
  if (diffDays === 1) return 'Yesterday'
  if (diffDays < 7) return `${diffDays} days ago`
  if (diffDays < 30) return `${Math.floor(diffDays / 7)} weeks ago`
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
    if (!directory) {
      return
    }

    const controller = new AbortController()
    loadingRef.current = true
    setRepoState(null)

    const fetchState = async () => {
      try {
        setIsLoading(true)
        setLoadError(false)
        const state = await tauriApi.repo.checkState(directory)
        if (!controller.signal.aborted) {
          setRepoState(state)
        }
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

    fetchState()

    return () => {
      controller.abort()
    }
  }, [directory])

  useEffect(() => {
    if (!isOpen) return

    const handleClickOutside = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setIsOpen(false)
      }
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
          'flex items-center gap-2 text-sm text-slate-500 dark:text-slate-400',
          className
        )}
      >
        <RefreshCw className="h-4 w-4 animate-spin" />
        <span>Checking repository...</span>
      </div>
    )
  }

  if (loadError) {
    return (
      <div className={cn('flex flex-col gap-1 text-xs text-slate-400', className)}>
        <div className="flex items-center gap-2">
          <GitBranch className="h-3.5 w-3.5" />
          <span>Could not check repository state</span>
        </div>
        {errorMessage && (
          <div className="text-xs text-red-500 dark:text-red-400 ml-5 font-mono">
            Error: {errorMessage}
          </div>
        )}
      </div>
    )
  }

  if (!repoState?.isGitRepo) {
    return (
      <div className={cn('flex items-center gap-2 text-xs text-slate-400', className)}>
        <GitBranch className="h-3.5 w-3.5" />
        <span>Not a git repository — worktrees unavailable</span>
      </div>
    )
  }

  const selectedBranch = repoState.branches.find((b) => b.name === value)

  const handleSelect = (branchName: string) => {
    if (value === branchName) {
      onChange(undefined)
    } else {
      onChange(branchName)
    }
    setIsOpen(false)
  }

  const handleClear = () => {
    onChange(undefined)
    setIsOpen(false)
  }

  return (
    <div ref={containerRef} className={cn('relative', className)}>
      <button
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        className={cn(
          'w-full flex items-center justify-between gap-2 px-3 py-2 rounded-lg text-sm transition-colors cursor-pointer',
          'bg-white/5 dark:bg-slate-800/50 border border-slate-200/50 dark:border-slate-700/50',
          'hover:bg-white/10 dark:hover:bg-slate-700/50',
          value && 'border-blue-500/50 dark:border-blue-500/30 bg-blue-500/5 dark:bg-blue-500/10'
        )}
      >
        <div className="flex items-center gap-2">
          <GitBranch className="h-4 w-4 text-slate-400" />
          {selectedBranch ? (
            <span className="text-slate-900 dark:text-slate-100">{selectedBranch.name}</span>
          ) : (
            <span className="text-slate-500 dark:text-slate-400">Select branch (optional)</span>
          )}
        </div>
        <ChevronDown
          className={cn('h-4 w-4 text-slate-400 transition-transform', isOpen && 'rotate-180')}
        />
      </button>

      {isOpen && (
        <div className="absolute z-50 mt-1 w-full max-h-64 overflow-auto rounded-lg border border-slate-200/50 dark:border-slate-700/50 bg-white dark:bg-slate-800 shadow-lg">
          <div className="sticky top-0 z-10 border-b border-slate-200/50 dark:border-slate-700/50 bg-white dark:bg-slate-800 px-2 py-1.5">
            <div className="relative">
              <Search
                size={12}
                className="absolute left-2.5 top-1/2 -translate-y-1/2 text-slate-400 pointer-events-none"
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
                placeholder="Filter branches..."
                className="w-full pl-7 pr-7 py-1.5 text-xs rounded-md bg-slate-100 dark:bg-slate-900 border border-transparent focus:border-blue-500/40 focus:outline-none text-slate-900 dark:text-slate-100 placeholder:text-slate-400"
              />
              {search && (
                <button
                  type="button"
                  onClick={() => setSearch('')}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 cursor-pointer"
                >
                  <X size={11} />
                </button>
              )}
            </div>
          </div>

          <button
            type="button"
            onClick={handleClear}
            className={cn(
              'w-full flex items-center gap-2 px-3 py-2 text-sm text-slate-500 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-700 cursor-pointer',
              !value && 'bg-slate-100 dark:bg-slate-700'
            )}
          >
            <span>No branch (work in directory)</span>
          </button>

          {repoState.isDirty && (
            <div className="px-3 py-2 text-xs text-amber-600 dark:text-amber-400 bg-amber-500/10">
              Repository has uncommitted changes. Please commit or stash before selecting a branch.
            </div>
          )}

          {filteredLocal.length > 0 && (
            <div className="border-t border-slate-200/50 dark:border-slate-700/50">
              <div className="px-3 py-1.5 text-xs font-medium text-slate-500 dark:text-slate-400 bg-slate-50 dark:bg-slate-900">
                Local branches
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
                      'w-full flex items-center justify-between px-3 py-2 text-sm hover:bg-slate-100 dark:hover:bg-slate-700 cursor-pointer',
                      value === branch.name && 'bg-blue-500/10 dark:bg-blue-500/20',
                      repoState.isDirty && !isCurrent && 'opacity-50 cursor-not-allowed'
                    )}
                  >
                    <div className="flex items-center gap-2">
                      {isCurrent && (
                        <span className="text-xs px-1.5 py-0.5 rounded bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 font-medium">
                          current
                        </span>
                      )}
                      <span
                        className={cn(
                          'text-slate-900 dark:text-slate-100',
                          isCurrent && 'font-medium'
                        )}
                      >
                        {branch.name}
                      </span>
                    </div>
                    {branch.lastCommitMessage && (
                      <span className="text-xs text-slate-500 dark:text-slate-400 truncate max-w-32 ml-2">
                        {branch.lastCommitMessage}
                      </span>
                    )}
                  </button>
                )
              })}
            </div>
          )}

          {filteredRemote.length > 0 && (
            <div className="border-t border-slate-200/50 dark:border-slate-700/50">
              <div className="px-3 py-1.5 text-xs font-medium text-slate-500 dark:text-slate-400 bg-slate-50 dark:bg-slate-900">
                Remote branches
              </div>
              {filteredRemote.map((branch) => (
                <button
                  key={branch.name}
                  type="button"
                  disabled={repoState.isDirty}
                  onClick={() => handleSelect(branch.name)}
                  className={cn(
                    'w-full flex items-center justify-between px-3 py-2 text-sm hover:bg-slate-100 dark:hover:bg-slate-700 cursor-pointer',
                    value === branch.name && 'bg-blue-500/10 dark:bg-blue-500/20',
                    repoState.isDirty && 'opacity-50 cursor-not-allowed'
                  )}
                >
                  <div className="flex flex-col items-start">
                    <span className="text-slate-900 dark:text-slate-100">{branch.name}</span>
                    {!branch.isCheckedOutLocally && (
                      <span className="text-xs text-blue-500">
                        Will create local tracking branch
                      </span>
                    )}
                  </div>
                  {branch.lastCommitDate && (
                    <span className="text-xs text-slate-500 dark:text-slate-400">
                      {formatDate(branch.lastCommitDate)}
                    </span>
                  )}
                </button>
              ))}
            </div>
          )}

          {search && filteredLocal.length === 0 && filteredRemote.length === 0 && (
            <div className="px-3 py-4 text-xs text-slate-400 text-center">
              No branches match &ldquo;{search}&rdquo;
            </div>
          )}
        </div>
      )}
    </div>
  )
}
