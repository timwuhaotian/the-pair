import { invoke } from '@tauri-apps/api/core'
import type { CreatePairInput, RepoState } from '../types'
import type { BranchInfo } from '../types'
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

const isTauriRuntime = () => typeof window !== 'undefined' && '__TAURI__' in window

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
        return mockRepoState
      }
      return await invokeTauri('repo_check_state', { directory })
    },
    listBranches: async (directory: string): Promise<BranchInfo[]> => {
      if (!isTauri) {
        return mockRepoState.branches
      }
      return await invokeTauri('repo_list_branches', { directory })
    }
  }
}

export { isTauri }
