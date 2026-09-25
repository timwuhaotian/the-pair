import { invoke, isTauri as coreIsTauri } from '@tauri-apps/api/core'
import type { CreatePairInput, RepoState } from '../types'
import type { BranchInfo, ConfigRecommendation, InsightsSummary } from '../types'
import { mockRepoState } from './mock-data'

export interface TauriPair {
  pairId: string
  name: string
  directory: string
  status: string
  mentorProvider: string
  mentorModel: string
  executorProvider: string
  executorModel: string
  createdAt: number
  branch?: string
  repoPath?: string
  worktreePath?: string
}

/**
 * True inside the Tauri webview. `window.__TAURI__` only exists with
 * `app.withGlobalTauri`, which this app does not enable, so detect the runtime
 * the way `@tauri-apps/api` does (`globalThis.isTauri`) plus the IPC internals
 * object every Tauri 2 webview injects.
 */
export function detectTauriRuntime(
  target: object | undefined = typeof window !== 'undefined' ? window : undefined
): boolean {
  if (!target) return false
  if ((target as { isTauri?: unknown }).isTauri === true) return true
  return '__TAURI_INTERNALS__' in target
}

const isTauriRuntime = (): boolean => coreIsTauri() || detectTauriRuntime()

const requireTauriRuntime = () => {
  if (!isTauriRuntime()) {
    throw new Error('Not running in Tauri')
  }
}

const invokeTauri = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
  requireTauriRuntime()
  return await invoke<T>(command, args)
}

const isTauri = isTauriRuntime()

export const tauriApi = {
  pair: {
    create: async (input: CreatePairInput): Promise<TauriPair> => {
      return await invokeTauri('pair_create', { input })
    },
    list: async (): Promise<TauriPair[]> => {
      return await invokeTauri('pair_list')
    },
    delete: async (pairId: string): Promise<void> => {
      return await invokeTauri('pair_delete', { pairId })
    },
    pause: async (pairId: string): Promise<void> => {
      return await invokeTauri('pair_pause', { pairId })
    },
    killProcess: async (pairId: string, role: string): Promise<void> => {
      return await invokeTauri('kill_process', { pairId, role })
    }
  },
  repo: {
    checkState: async (directory: string): Promise<RepoState> => {
      if (!isTauri) {
        return { ...mockRepoState }
      }
      return await invokeTauri('repo_check_state', { directory })
    },
    listBranches: async (directory: string): Promise<BranchInfo[]> => {
      if (!isTauri) {
        return [...mockRepoState.branches]
      }
      return await invokeTauri('repo_list_branches', { directory })
    },
    getFileDiff: async (directory: string, filePath: string, status: string): Promise<string> => {
      return await invokeTauri('git_get_file_diff', { directory, filePath, status })
    }
  },
  insights: {
    getSummary: async (): Promise<InsightsSummary> => {
      return await invokeTauri('get_insights_summary')
    },
    getRecommendation: async (taskText: string): Promise<ConfigRecommendation[]> => {
      return await invokeTauri('get_recommendation', { taskText })
    }
  }
}

export { isTauri }
