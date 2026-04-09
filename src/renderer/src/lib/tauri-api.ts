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

const isTauri = '__TAURI__' in window

export const tauriApi = {
  pair: {
    create: async (input: CreatePairInput): Promise<TauriPair> => {
      if (!isTauri) throw new Error('Not running in Tauri')
      return await invoke('pair_create', { input })
    },
    list: async (): Promise<TauriPair[]> => {
      if (!isTauri) throw new Error('Not running in Tauri')
      return await invoke('pair_list')
    },
    delete: async (pairId: string): Promise<void> => {
      if (!isTauri) throw new Error('Not running in Tauri')
      return await invoke('pair_delete', { pairId })
    },
    pause: async (pairId: string): Promise<void> => {
      if (!isTauri) throw new Error('Not running in Tauri')
      return await invoke('pair_pause', { pairId })
    },
    killProcess: async (pairId: string, role: string): Promise<void> => {
      if (!isTauri) throw new Error('Not running in Tauri')
      return await invoke('kill_process', { pairId, role })
    }
  },
  repo: {
    checkState: async (directory: string): Promise<RepoState> => {
      if (!isTauri) {
        return mockRepoState
      }
      return (await invoke('repo_check_state', { directory })) as RepoState
    },
    listBranches: async (directory: string): Promise<BranchInfo[]> => {
      if (!isTauri) {
        return mockRepoState.branches
      }
      return await invoke('repo_list_branches', { directory })
    }
  }
}

export { isTauri }
