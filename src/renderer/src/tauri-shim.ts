import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

const api = {
  app: {
    restart: () => invoke('app_restart') as Promise<unknown>
  },
  pair: {
    create: (input: unknown) => invoke('pair_create', { input }) as Promise<unknown>,
    assignTask: (pairId: string, input: unknown) =>
      invoke('pair_assign_task', { pairId, input }) as Promise<unknown>,
    updateModels: (pairId: string, input: unknown) =>
      invoke('pair_update_models', { pairId, input }) as Promise<unknown>,
    pause: (pairId: string) => invoke('pair_pause', { pairId }) as Promise<unknown>,
    resume: (pairId: string) => invoke('pair_resume', { pairId }) as Promise<unknown>,
    stop: (pairId: string) => invoke('pair_pause', { pairId }) as Promise<unknown>,
    delete: (pairId: string) => invoke('pair_delete', { pairId }) as Promise<unknown>,
    retryTurn: (pairId: string) => invoke('pair_retry_turn', { pairId }) as Promise<unknown>,
    killProcess: (pairId: string, role: string) =>
      invoke('kill_process', { pairId, role }) as Promise<unknown>,
    list: () => invoke('pair_list') as Promise<unknown>,
    getMessages: (pairId: string) => invoke('pair_get_messages', { pairId }) as Promise<unknown>,
    getState: (pairId: string) => invoke('pair_get_state', { pairId }) as Promise<unknown>,
    onCreated: (callback: (data: unknown) => void) =>
      listen('pair:created', (e) => callback(e.payload)) as Promise<unknown>,
    onStopped: (callback: (data: unknown) => void) =>
      listen('pair:stopped', (e) => callback(e.payload)) as Promise<unknown>,
    onMessage: (callback: (data: unknown) => void) =>
      listen('pair:message', (e) => callback(e.payload)) as Promise<unknown>,
    onState: (callback: (data: unknown) => void) =>
      listen('pair:state', (e) => callback(e.payload)) as Promise<unknown>,
    onHandoff: (callback: (data: unknown) => void) =>
      listen('pair:handoff', (e) => callback(e.payload)) as Promise<unknown>
  },
  session: {
    saveSnapshot: (input: unknown) =>
      invoke('session_save_snapshot', { input }) as Promise<unknown>,
    loadAllPairs: () => invoke('load_all_pairs') as Promise<unknown>,
    listRecoverable: () => invoke('list_recoverable_sessions') as Promise<unknown>,
    deleteRecoverable: (pairId: string) =>
      invoke('delete_recoverable_session', { pairId }) as Promise<unknown>,
    restore: (pairId: string, continueRun = true) =>
      invoke('restore_session', { input: { pairId, continueRun } }) as Promise<unknown>
  },
  config: {
    getModels: () => invoke('config_get_models') as Promise<unknown>,
    getCachedModels: () => invoke('config_get_cached_models') as Promise<unknown>,
    refreshModels: () => invoke('config_refresh_models') as Promise<unknown>,
    getProviders: () => invoke('config_get_providers') as Promise<unknown>,
    read: () => invoke('config_read') as Promise<unknown>,
    openFile: () => invoke('config_open_file') as Promise<unknown>,
    getVersion: () => Promise.resolve(__APP_VERSION__)
  },
  file: {
    listFiles: (options: { pairId?: string; directory?: string }) =>
      invoke('file_list_files', { options }) as Promise<
        Array<{ path: string; type: 'file' | 'directory' }>
      >,
    parseMentions: (pairId: string, spec: string) =>
      invoke('file_parse_mentions', { pairId, spec }) as Promise<string>,
    readContent: (options: { pairId?: string; directory?: string; filePath: string }) =>
      invoke('file_read_content', { options }) as Promise<string>
  }
}

window.api = api
