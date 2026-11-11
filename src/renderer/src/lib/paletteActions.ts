import type { Pair } from '../store/usePairStore'

/**
 * An action the command palette (and, where applicable, the native menu bar)
 * can invoke. `available` gates the entry so context-only actions never show
 * when they cannot run.
 */
export interface PaletteAction {
  id: string
  /** i18n key for the label. */
  labelKey: string
  /** i18n key for the group header. */
  groupKey: string
  /** Display tokens for the shortcut hint, e.g. ["⌘", "K"]. */
  shortcut?: string[]
  available?: () => boolean
  run: () => void
}

import type { ShellSection } from '../components/AppShell'

export interface PaletteContext {
  pairs: Pair[]
  selectedPair: Pair | null
  openCreatePair: () => void
  openNewTask: () => void
  openPairSettings: () => void
  pauseSelectedPair: () => void
  resumeSelectedPair: () => void
  clearSelectedSession: () => void
  selectPair: (pairId: string | null) => void
  selectSection: (section: ShellSection) => void
  toggleSidebar: () => void
  toggleTheme: () => void
  openShortcuts: () => void
  checkForUpdates: () => void
}

export function buildPaletteActions(ctx: PaletteContext): PaletteAction[] {
  const { selectedPair } = ctx
  return [
    {
      id: 'pair.new',
      labelKey: 'palette.newPair',
      groupKey: 'palette.groups.pair',
      run: ctx.openCreatePair
    },
    {
      id: 'task.new',
      labelKey: 'palette.newTask',
      groupKey: 'palette.groups.pair',
      shortcut: ['⌘', 'N'],
      available: () => selectedPair !== null,
      run: ctx.openNewTask
    },
    {
      id: 'pair.pause',
      labelKey: 'palette.pausePair',
      groupKey: 'palette.groups.pair',
      shortcut: ['⌘', 'P'],
      available: () =>
        selectedPair !== null &&
        (selectedPair.status === 'Mentoring' ||
          selectedPair.status === 'Executing' ||
          selectedPair.status === 'Reviewing'),
      run: ctx.pauseSelectedPair
    },
    {
      id: 'pair.resume',
      labelKey: 'palette.resumePair',
      groupKey: 'palette.groups.pair',
      shortcut: ['⌘', '⇧', 'P'],
      available: () => selectedPair?.status === 'Paused',
      run: ctx.resumeSelectedPair
    },
    {
      id: 'pair.settings',
      labelKey: 'palette.pairSettings',
      groupKey: 'palette.groups.pair',
      available: () => selectedPair !== null,
      run: ctx.openPairSettings
    },
    {
      id: 'pair.clearSession',
      labelKey: 'palette.clearSession',
      groupKey: 'palette.groups.pair',
      available: () => selectedPair !== null && (selectedPair.messages.length ?? 0) > 0,
      run: ctx.clearSelectedSession
    },
    {
      id: 'nav.pairs',
      labelKey: 'palette.goPairs',
      groupKey: 'palette.groups.go',
      run: () => ctx.selectSection('pairs')
    },
    {
      id: 'nav.activity',
      labelKey: 'palette.goActivity',
      groupKey: 'palette.groups.go',
      run: () => ctx.selectSection('activity')
    },
    {
      id: 'nav.models',
      labelKey: 'palette.goModels',
      groupKey: 'palette.groups.go',
      run: () => ctx.selectSection('models')
    },
    {
      id: 'nav.insights',
      labelKey: 'palette.goInsights',
      groupKey: 'palette.groups.go',
      run: () => ctx.selectSection('insights')
    },
    {
      id: 'view.toggleSidebar',
      labelKey: 'palette.toggleSidebar',
      groupKey: 'palette.groups.view',
      shortcut: ['⌘', '⌃', 'B'],
      run: ctx.toggleSidebar
    },
    {
      id: 'view.toggleTheme',
      labelKey: 'palette.toggleTheme',
      groupKey: 'palette.groups.view',
      run: ctx.toggleTheme
    },
    {
      id: 'view.shortcuts',
      labelKey: 'palette.keyboardShortcuts',
      groupKey: 'palette.groups.view',
      shortcut: ['⇧', '?'],
      run: ctx.openShortcuts
    },
    {
      id: 'app.checkUpdates',
      labelKey: 'palette.checkUpdates',
      groupKey: 'palette.groups.app',
      run: ctx.checkForUpdates
    }
  ]
}

/** Pair entries: jumping straight to a pair is the palette's fastest path. */
export function buildPairJumpActions(
  pairs: Pair[],
  selectPair: (pairId: string | null) => void
): PaletteAction[] {
  return pairs.map((pair) => ({
    id: `go.pair.${pair.id}`,
    labelKey: 'palette.openPair',
    groupKey: 'palette.groups.go',
    available: () => true,
    run: () => selectPair(pair.id)
  }))
}
