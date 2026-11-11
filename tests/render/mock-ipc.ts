/**
 * Mock Tauri IPC for component unit tests: one global handler covering the
 * commands the create / assign / chrome components reach through window.api
 * or lib/tauri-api. `state.repo` lets each test script the repository state
 * (e.g. a committed git repo that defaults the worktree toggle on).
 */
import { mockIPC } from '@tauri-apps/api/mocks'
// The app installs window.api via this shim in main.tsx; components read it.
import '../../src/renderer/src/tauri-shim.ts'
import type { ConfigRecommendation } from '../../src/renderer/src/types.ts'

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

/**
 * Registry state for the models section. `candidates` is what `accounts_scan`
 * answers; `providers` is what the registry holds.
 */
export const registry = {
  providers: [
    {
      id: 'anthropic-1',
      protocol: 'anthropic',
      baseUrl: 'https://api.anthropic.com',
      vendor: 'anthropic',
      displayName: 'Anthropic',
      readyAccountCount: 1,
      accounts: [
        {
          id: 'anthropic-1-a',
          kind: 'apiKey',
          label: 'ANTHROPIC_API_KEY',
          source: 'claude',
          status: 'ready',
          headers: [],
          hasSecret: true,
          maskedSecret: 'sk-ant…mnop'
        }
      ],
      models: [
        {
          id: 'claude-sonnet-4-5-20250929',
          displayName: 'Claude Sonnet 4.5',
          contextWindow: 200000,
          maxOutput: 64000,
          efforts: [],
          source: 'preset'
        }
      ]
    },
    {
      id: 'openai-1',
      protocol: 'responses',
      baseUrl: 'https://api.openai.com/v1',
      vendor: 'openai',
      displayName: 'OpenAI',
      readyAccountCount: 0,
      accounts: [],
      models: []
    }
  ],
  candidates: [
    {
      sourceId: 'kimi',
      sourceName: 'Kimi Code',
      configPath: '/home/dev/.kimi-code/config.toml',
      accountCount: 1,
      newProviderCount: 1,
      notes: [],
      providers: [
        {
          protocol: 'openaiChat',
          baseUrl: 'https://api.moonshot.cn/v1',
          vendor: 'moonshot',
          displayName: 'Moonshot (Kimi Code)',
          alreadyImported: false,
          models: [],
          accounts: [
            {
              kind: 'apiKey',
              label: 'moonshot',
              source: 'kimi',
              status: 'ready',
              maskedSecret: 'sk-ki…abcd',
              headers: []
            }
          ]
        }
      ]
    }
  ],
  loginServices: [
    { id: 'chatgpt', displayName: 'ChatGPT', needsBrowser: true, needsKey: false },
    { id: 'commandcode', displayName: 'Command Code', needsBrowser: true, needsKey: true },
    { id: 'opencode-go', displayName: 'OpenCode Go', needsBrowser: false, needsKey: true },
    { id: 'muse', displayName: 'Muse Code', needsBrowser: false, needsKey: true }
  ],
  calls: [] as Array<{ cmd: string; args: unknown }>
}

export interface MockState {
  repo: MockRepoState
  /** When true, repo_check_state rejects (unreadable repository). */
  failRepoCheck: boolean
  /** What get_recommendation answers; null means "no recommendation". */
  recommendation: ConfigRecommendation[] | null
}

export const state: MockState = {
  repo: mockRepoState.committedRepo,
  failRepoCheck: false,
  recommendation: null
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
    case 'accounts_list':
      return registry.providers
    case 'accounts_scan':
      registry.calls.push({ cmd, args })
      return registry.candidates
    case 'accounts_commit_import':
      registry.calls.push({ cmd, args })
      return registry.providers
    case 'accounts_login_services':
      return registry.loginServices
    case 'accounts_launch_oauth':
      registry.calls.push({ cmd, args })
      return 'session-1'
    case 'accounts_oauth_status':
      return { state: 'waiting', service: 'chatgpt', waitingMs: 1200 }
    case 'accounts_cancel_oauth':
      return undefined
    case 'accounts_add_provider':
    case 'accounts_set_account_key':
    case 'accounts_delete_account':
      registry.calls.push({ cmd, args })
      return registry.providers
    case 'config_get_cached_models':
    case 'config_get_models':
      return models
    case 'repo_check_state':
      if (state.failRepoCheck) throw new Error('repo state unreadable')
      return state.repo
    case 'repo_list_branches':
      return state.repo.branches
    case 'get_recommendation':
      return state.recommendation ?? []
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
