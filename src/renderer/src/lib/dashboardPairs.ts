import { type Pair } from '../store/usePairStore'
import { isPairActive } from './pairStatus'

type DashboardPairGroupKey = 'attention' | 'active' | 'paused' | 'ready' | 'finished'

export interface DashboardPairGroup {
  key: DashboardPairGroupKey
  titleKey: string
  pairs: Pair[]
}

export interface DashboardWorkspaceGroup {
  /** Absolute workspace directory used for grouping. */
  directory: string
  /** Short folder name (last path segment) for display. */
  shortName: string
  pairs: Pair[]
  statusGroups: DashboardPairGroup[]
}

export interface DashboardPairInsights {
  needsAttention: number
  running: number
  modifiedFiles: number
  cpuUsage: number
  memUsage: number
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
  const cleaned = directory.replace(/\/+$/, '')
  const segments = cleaned.split('/')
  return segments[segments.length - 1] || cleaned || directory
}

export function buildWorkspaceGroups(pairs: Pair[]): DashboardWorkspaceGroup[] {
  const buckets = new Map<string, Pair[]>()
  const orderedKeys: string[] = []

  for (const pair of pairs) {
    const key = pair.directory || '—'
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
      pairs: groupPairs,
      statusGroups: buildPairGroups(groupPairs)
    }
  })
}

export function buildPairInsights(pairs: Pair[]): DashboardPairInsights {
  const cpuUsage = pairs.reduce((total, pair) => total + pair.cpuUsage, 0)
  const memUsage = pairs.reduce((total, pair) => total + pair.memUsage, 0)
  const workspaceCount = new Set(pairs.map((pair) => pair.directory)).size

  return {
    needsAttention: pairs.filter(
      (pair) => pair.status === 'Error' || pair.status === 'Awaiting Human Review'
    ).length,
    running: pairs.filter((pair) => isPairActive(pair.status)).length,
    modifiedFiles: pairs.reduce((total, pair) => total + pair.modifiedFiles.length, 0),
    cpuUsage: Number(cpuUsage.toFixed(1)),
    memUsage: Math.round(memUsage),
    workspaceCount
  }
}
