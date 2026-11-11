/**
 * Browser-side mock of the Tauri IPC layer for the headless e2e suite
 * (chromium against the vite dev server). Injected via page.addInitScript
 * before any app code runs; test state arrives on window.__MOCK_STATE__.
 *
 * Records every call on window.__MOCK_CALLS__ so specs can assert the exact
 * payloads the UI sends (e.g. pair_create's useWorktree flag).
 */
;(() => {
  const state = window.__MOCK_STATE__ || {}
  const calls = []
  window.__MOCK_CALLS__ = calls

  const defaultRepo = {
    isGitRepo: true,
    isDirty: false,
    currentBranch: 'main',
    branches: [
      {
        name: 'main',
        isLocal: true,
        isRemote: false,
        lastCommitMessage: 'init',
        lastCommitDate: 1700000000,
        isCheckedOutLocally: true
      },
      {
        name: 'feature/login',
        isLocal: true,
        isRemote: false,
        lastCommitMessage: 'wip',
        lastCommitDate: 1700000100,
        isCheckedOutLocally: false
      }
    ]
  }
  const nonGitRepo = {
    isGitRepo: false,
    isDirty: false,
    currentBranch: null,
    branches: []
  }
  const repo = state.repo || defaultRepo

  // The direct-connection model registry (`accounts_list`): every endpoint the
  // app can talk to, the accounts behind it and the models it lists. One
  // endpoint is signed in and one is not — the second is what the Models
  // section and the picker's "needs credential" rows render.
  const providers = state.providers || [
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
          source: 'preset',
          price: { inputPerMillion: 3, outputPerMillion: 15 }
        },
        {
          id: 'claude-opus-4-1-20250805',
          displayName: 'Claude Opus 4.1',
          contextWindow: 200000,
          maxOutput: 32000,
          efforts: [],
          source: 'preset',
          price: { inputPerMillion: 15, outputPerMillion: 75 }
        }
      ]
    },
    {
      id: 'openai-responses-1',
      protocol: 'responses',
      baseUrl: 'https://api.openai.com/v1',
      vendor: 'openai',
      displayName: 'OpenAI',
      readyAccountCount: 0,
      accounts: [
        {
          id: 'openai-responses-1-a',
          kind: 'apiKey',
          label: 'OPENAI_API_KEY',
          source: 'codex',
          status: 'needsKey',
          headers: [],
          hasSecret: false
        }
      ],
      models: [
        {
          id: 'gpt-5.1',
          displayName: 'GPT-5.1',
          contextWindow: 400000,
          maxOutput: 128000,
          efforts: ['low', 'medium', 'high'],
          defaultEffort: 'medium',
          source: 'preset',
          price: { inputPerMillion: 1.25, outputPerMillion: 10 }
        },
        {
          id: 'gpt-5.1-codex-max',
          displayName: 'GPT-5.1 Codex Max',
          contextWindow: 400000,
          maxOutput: 128000,
          efforts: ['low', 'medium', 'high', 'xhigh'],
          defaultEffort: 'medium',
          source: 'preset',
          price: { inputPerMillion: 1.25, outputPerMillion: 10 }
        }
      ]
    }
  ]

  const worktree = state.worktree || {
    pairId: 'pair-e2e',
    directory: '/repo/.worktrees/pair-e2e',
    branch: null,
    repoPath: '/repo',
    worktreePath: '/repo/.worktrees/pair-e2e',
    worktreeBranch: 'the-pair/pair-e2e'
  }

  const candidates = state.candidates || []
  const loginServices = [
    { id: 'chatgpt', displayName: 'ChatGPT', needsBrowser: true, needsKey: false },
    { id: 'commandcode', displayName: 'Command Code', needsBrowser: true, needsKey: true },
    { id: 'opencode-go', displayName: 'OpenCode Go', needsBrowser: false, needsKey: true },
    { id: 'muse', displayName: 'Muse Code', needsBrowser: false, needsKey: true }
  ]

  const invoke = async (cmd, args) => {
    calls.push({ cmd, args })
    switch (cmd) {
      case 'accounts_list':
        return providers
      case 'accounts_scan':
        return candidates
      case 'accounts_commit_import':
        return providers
      case 'accounts_login_services':
        return loginServices
      case 'accounts_launch_oauth':
        return 'session-e2e'
      case 'accounts_oauth_status':
        return { state: 'waiting', service: 'chatgpt', waitingMs: 500 }
      case 'accounts_cancel_oauth':
      case 'accounts_prefer_route':
        return undefined
      case 'accounts_add_provider':
      case 'accounts_set_account_key':
      case 'accounts_delete_account':
        return providers
      case 'repo_check_state': {
        const dir = args && args.directory
        if (dir && state.nonGitDir && String(dir).includes(state.nonGitDir)) return nonGitRepo
        return repo
      }
      case 'repo_list_branches':
        return repo.branches
      case 'pair_list':
        return []
      case 'pair_get_state':
        return null
      case 'pair_get_messages':
        return []
      case 'pair_create':
        return worktree
      case 'pair_assign_task':
        return worktree
      case 'pair_update_models':
        return args
      case 'get_recommendation':
        return []
      case 'get_insights_summary':
        return { totalRuns: 0, successfulRuns: 0, successRate: 0, combos: [] }
      case 'skill_discover':
      case 'discover_skills':
        return []
      case 'file_list_files':
        return []
      case 'skill_read_content':
        return ''
      case 'load_all_pairs':
        return state.snapshots ?? []
      case 'list_recoverable_sessions':
        return []
      case 'git_commit_changes':
        return { sha: 'abc1234', filesCommitted: 1 }
      case 'git_push_changes': {
        const pair = (state.snapshots ?? []).find((s) => s && s.directory === args?.directory)
        return {
          branch: pair?.worktreeBranch ?? 'the-pair/pair-e2e',
          remote: 'origin',
          upToDate: false
        }
      }
      default:
        return undefined
    }
  }

  window.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback: (callback) => {
      const id = Math.floor(Math.random() * 1e9)
      window[`_${id}`] = callback
      return id
    },
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } }
  }
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: () => undefined,
    listen: (event, handler, options) => {
      const id = Math.floor(Math.random() * 1e9)
      window[`_${id}`] = (e) => handler(e)
      invoke('plugin:event|listen', {
        msg: { event, id, target: options?.target }
      })
      return id
    },
    emit: (event, payload, target) =>
      invoke('plugin:event|emit', { msg: { event, payload, target } })
  }
})()
