/**
 * Mock Tauri IPC for component unit tests: one global handler covering the
 * commands the create / assign / chrome components reach through window.api
 * or lib/tauri-api. `state.repo` lets each test script the repository state
 * (e.g. a committed git repo that defaults the worktree toggle on).
 */
import { mockIPC } from '@tauri-apps/api/mocks'
// The app installs window.api via this shim in main.tsx; components read it.
import '../../src/renderer/src/tauri-shim.ts'

export interface MockRepoState {
  isGitRepo: boolean
  isDirty: boolean
  currentBranch: string | null
  branches: Array<{
    name: string
    isLocal: boolean
    isRemote: boolean
    lastCommitMessage?: string
    lastCommitDate?: number
    isCheckedOutLocally?: boolean
  }>
}

export const mockRepoState = {
  committedRepo: {
    isGitRepo: true,
    isDirty: false,
    currentBranch: 'main',
    branches: [
      {
        name: 'main',
        isLocal: true,
        isRemote: false,
        lastCommitMessage: 'init',
        lastCommitDate: 1_700_000_000,
        isCheckedOutLocally: true
      },
      {
        name: 'feature/login',
        isLocal: true,
        isRemote: false,
        lastCommitMessage: 'wip',
        lastCommitDate: 1_700_000_100,
        isCheckedOutLocally: false
      }
    ]
  } satisfies MockRepoState,
  emptyRepo: {
    isGitRepo: false,
    isDirty: false,
    currentBranch: null,
    branches: []
  } satisfies MockRepoState
}

const models = [
  {
    provider: 'grok',
    modelId: 'fast',
    displayName: 'Fast',
    providerLabel: 'Grok',
    sourceProviderLabel: 'xAI',
    billingKind: 'byok',
    billingLabel: 'BYOK',
    accessLabel: 'Ready',
    availabilityStatus: 'ready',
    availabilityReason: undefined,
    supportsPairExecution: true,
    recommendedRoles: ['mentor', 'executor'],
    reasoningEffortLevels: []
  }
]

export interface MockState {
  repo: MockRepoState
  /** When true, repo_check_state rejects (unreadable repository). */
  failRepoCheck: boolean
}

export const state: MockState = {
  repo: mockRepoState.committedRepo,
  failRepoCheck: false
}

export interface MockCommitPush {
  commit?: { sha: string; filesCommitted: number } | { error: string }
  push?: { branch: string; remote: string; upToDate: boolean } | { error: string }
  calls: Array<{ cmd: string; args: unknown }>
}

export const commitPush: MockCommitPush = { calls: [] }

mockIPC((cmd, args) => {
  commitPush.calls.push({ cmd: cmd as string, args: args as unknown })
  switch (cmd) {
    case 'git_commit_changes': {
      const result = commitPush.commit ?? { sha: 'abc1234', filesCommitted: 1 }
      if ('error' in result) throw new Error(result.error)
      return result
    }
    case 'git_push_changes': {
      const result = commitPush.push ?? {
        branch: 'the-pair/pair-ui',
        remote: 'origin',
        upToDate: false
      }
      if ('error' in result) throw new Error(result.error)
      return result
    }
    case 'config_get_cached_models':
    case 'config_get_models':
      return models
    case 'repo_check_state':
      if (state.failRepoCheck) throw new Error('repo state unreadable')
      return state.repo
    case 'repo_list_branches':
      return state.repo.branches
    case 'get_recommendation':
      return []
    case 'get_insights_summary':
      return { runs: 0, interventions: 0, combos: [] }
    case 'skill_discover':
    case 'discover_skills':
      return []
    case 'file_list_files':
      return []
    case 'skill_read_content':
      return ''
    case 'pair_list':
      return []
    default:
      return undefined
  }
})
