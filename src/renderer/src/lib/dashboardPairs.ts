import { type Pair } from '../store/usePairStore'
import { isPairActive } from './pairStatus'
import { compactPath, derivePairNameFromDirectory } from './workspace'

type DashboardPairGroupKey = 'attention' | 'active' | 'paused' | 'ready' | 'finished'

export interface DashboardPairGroup {
  key: DashboardPairGroupKey
  titleKey: string
  pairs: Pair[]
}

export interface DashboardWorkspaceGroup {
  /** Absolute workspace directory used for grouping: the repository a pair
   *  belongs to, so pairs in per-task worktrees of one repo share a group. */
  directory: string
  /** Short folder name (last path segment) for display. */
  shortName: string
  /** The directory with its home prefix and middle elided, tail kept. */
  displayPath: string
  pairs: Pair[]
  statusGroups: DashboardPairGroup[]
}

export interface DashboardPairInsights {
  needsAttention: number
  running: number
  modifiedFiles: number
  workspaceCount: number
}

export function buildPairGroups(pairs: Pair[]): DashboardPairGroup[] {
  const groups: DashboardPairGroup[] = [
    {
      key: 'attention',
      titleKey: 'dashboard.groups.attention',
      pairs: pairs.filter((p) => p.status === 'Error' || p.status === 'Awaiting Human Review')
    },
    {
      key: 'active',
      titleKey: 'dashboard.groups.active',
      pairs: pairs.filter((p) => isPairActive(p.status))
    },
    {
      key: 'paused',
      titleKey: 'dashboard.groups.paused',
      pairs: pairs.filter((p) => p.status === 'Paused')
    },
    {
      key: 'ready',
      titleKey: 'dashboard.groups.ready',
      pairs: pairs.filter((p) => p.status === 'Idle')
    },
    {
      key: 'finished',
      titleKey: 'dashboard.groups.completed',
      pairs: pairs.filter((p) => p.status === 'Finished')
    }
  ]

  return groups.filter((group) => group.pairs.length > 0)
}

function shortenWorkspace(directory: string): string {
  // Handles both `/` and Windows `\` separators.
  return derivePairNameFromDirectory(directory) || directory
}

/** The workspace a pair belongs to: its repository when it runs in a worktree. */
export function workspaceKey(pair: Pair): string {
  return pair.repoPath || pair.directory || '—'
}

export function buildWorkspaceGroups(pairs: Pair[]): DashboardWorkspaceGroup[] {
  const buckets = new Map<string, Pair[]>()
  const orderedKeys: string[] = []

  for (const pair of pairs) {
    const key = workspaceKey(pair)
    if (!buckets.has(key)) {
      buckets.set(key, [])
      orderedKeys.push(key)
    }
    buckets.get(key)!.push(pair)
  }

  return orderedKeys.map((directory) => {
    const groupPairs = buckets.get(directory) ?? []
    return {
      directory,
      shortName: shortenWorkspace(directory),
      displayPath: compactPath(directory),
      pairs: groupPairs,
      statusGroups: buildPairGroups(groupPairs)
    }
  })
}

export function buildPairInsights(pairs: Pair[]): DashboardPairInsights {
  const workspaceCount = new Set(pairs.map(workspaceKey)).size

  return {
    needsAttention: pairs.filter(
      (pair) => pair.status === 'Error' || pair.status === 'Awaiting Human Review'
    ).length,
    running: pairs.filter((pair) => isPairActive(pair.status)).length,
    modifiedFiles: pairs.reduce((total, pair) => total + pair.modifiedFiles.length, 0),
    workspaceCount
  }
}
