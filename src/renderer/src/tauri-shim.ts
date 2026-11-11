import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  AddProviderInput,
  CandidateView,
  LoginServiceView,
  OAuthStatusView,
  ProviderView
} from './types'

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
  },
  repo: {
    getFileDiff: (directory: string, filePath: string, status: string) =>
      invoke('git_get_file_diff', { directory, filePath, status }) as Promise<string>,
    commitChanges: (directory: string, message: string) =>
      invoke('git_commit_changes', { directory, message }) as Promise<{
        sha: string
        filesCommitted: number
      }>,
    pushChanges: (directory: string) =>
      invoke('git_push_changes', { directory }) as Promise<{
        branch: string
        remote: string
        upToDate: boolean
      }>
  },
  skill: {
    discover: (projectDir?: string) =>
      invoke('discover_skills', { projectDir: projectDir ?? null }) as Promise<
        Array<{ name: string; description: string; source: string }>
      >,
    readContent: (name: string, projectDir?: string) =>
      invoke('skill_read_content', { name, projectDir: projectDir ?? null }) as Promise<string>,
    refresh: (projectDir?: string) =>
      invoke('skill_refresh', { projectDir: projectDir ?? null }) as Promise<
        Array<{ name: string; description: string; source: string }>
      >
  },
  models: {
    list: () => invoke('accounts_list') as Promise<ProviderView[]>,
    scan: (sourceIds?: string[]) =>
      invoke('accounts_scan', { sourceIds: sourceIds ?? null }) as Promise<CandidateView[]>,
    commitImport: (sourceIds: string[]) =>
      invoke('accounts_commit_import', { sourceIds }) as Promise<ProviderView[]>,
    addProvider: (input: AddProviderInput) =>
      invoke('accounts_add_provider', { input }) as Promise<ProviderView[]>,
    setAccountKey: (input: { providerId: string; accountId: string; apiKey: string }) =>
      invoke('accounts_set_account_key', { input }) as Promise<ProviderView[]>,
    deleteAccount: (input: { providerId: string; accountId: string }) =>
      invoke('accounts_delete_account', { input }) as Promise<ProviderView[]>,
    preferRoute: (input: { role: 'mentor' | 'executor'; modelId: string; providerId: string }) =>
      invoke('accounts_prefer_route', { input }) as Promise<void>,
    loginServices: () => invoke('accounts_login_services') as Promise<LoginServiceView[]>,
    launchOAuth: (service: string, apiKey?: string) =>
      invoke('accounts_launch_oauth', { service, apiKey: apiKey ?? null }) as Promise<string>,
    oauthStatus: (sessionId: string) =>
      invoke('accounts_oauth_status', { sessionId }) as Promise<OAuthStatusView>,
    cancelOAuth: (sessionId: string) =>
      invoke('accounts_cancel_oauth', { sessionId }) as Promise<void>
  },
  insights: {
    recordIntervention: (input: { pairId: string; kind: string; outcome?: string }) =>
      invoke('record_intervention', input) as Promise<void>
  }
}

window.api = api
