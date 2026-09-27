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
  const models = state.models || [
    {
      provider: 'grok',
      modelId: 'fast',
      displayName: 'Fast',
      available: true,
      providerLabel: 'Grok',
      sourceProviderLabel: 'xAI',
      billingKind: 'byok',
      billingLabel: 'BYOK',
      accessLabel: 'Ready',
      availabilityStatus: 'ready',
      supportsPairExecution: true,
      recommendedRoles: ['mentor', 'executor'],
      reasoningEffortLevels: []
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

  const invoke = async (cmd, args) => {
    calls.push({ cmd, args })
    switch (cmd) {
      case 'config_get_cached_models':
      case 'config_get_models':
      case 'config_refresh_models':
        return models
      case 'config_get_providers':
        return []
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
        return { runs: 0, interventions: 0, combos: [] }
      case 'skill_discover':
      case 'discover_skills':
        return []
      case 'file_list_files':
        return []
      case 'skill_read_content':
        return ''
      case 'load_all_pairs':
        return []
      case 'list_recoverable_sessions':
        return []
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
