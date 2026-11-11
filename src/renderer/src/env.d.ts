/// <reference types="vite/client" />
// NOTE: this file must stay a global script (no top-level import/export), or
// the `Window` augmentation stops applying. Types are reached with `import()`
// type queries, which do not turn the file into a module.

interface Window {
  api: {
    app: {
      restart: () => Promise<unknown>
    }
    pair: {
      create: (input: unknown) => Promise<unknown>
      assignTask: (pairId: string, input: unknown) => Promise<unknown>
      updateModels: (pairId: string, input: unknown) => Promise<unknown>
      pause: (pairId: string) => Promise<unknown>
      resume: (pairId: string) => Promise<unknown>
      stop: (pairId: string) => Promise<unknown>
      delete: (pairId: string) => Promise<unknown>
      retryTurn: (pairId: string) => Promise<unknown>
      killProcess: (pairId: string, role: string) => Promise<unknown>
      list: () => Promise<unknown>
      getMessages: (pairId: string) => Promise<unknown>
      getState: (pairId: string) => Promise<unknown>
      onCreated: (callback: (data: unknown) => void) => Promise<unknown>
      onStopped: (callback: (data: unknown) => void) => Promise<unknown>
      onMessage: (callback: (data: unknown) => void) => Promise<unknown>
      onState: (callback: (data: unknown) => void) => Promise<unknown>
      onHandoff: (callback: (data: unknown) => void) => Promise<unknown>
    }
    session: {
      saveSnapshot: (input: unknown) => Promise<unknown>
      loadAllPairs: () => Promise<unknown>
      listRecoverable: () => Promise<unknown>
      deleteRecoverable: (pairId: string) => Promise<unknown>
      restore: (pairId: string, continueRun?: boolean) => Promise<unknown>
    }
    config: {
      getVersion: () => Promise<string>
    }
    models: {
      list: () => Promise<import('./types').ProviderView[]>
      scan: (sourceIds?: string[]) => Promise<import('./types').CandidateView[]>
      commitImport: (sourceIds: string[]) => Promise<import('./types').ProviderView[]>
      addProvider: (
        input: import('./types').AddProviderInput
      ) => Promise<import('./types').ProviderView[]>
      setAccountKey: (input: {
        providerId: string
        accountId: string
        apiKey: string
      }) => Promise<import('./types').ProviderView[]>
      deleteAccount: (input: {
        providerId: string
        accountId: string
      }) => Promise<import('./types').ProviderView[]>
      /** Tell the backend which endpoint a model was picked under for a role. */
      preferRoute: (input: {
        role: 'mentor' | 'executor'
        modelId: string
        providerId: string
      }) => Promise<void>
      loginServices: () => Promise<import('./types').LoginServiceView[]>
      launchOAuth: (service: string, apiKey?: string) => Promise<string>
      oauthStatus: (sessionId: string) => Promise<import('./types').OAuthStatusView>
      cancelOAuth: (sessionId: string) => Promise<void>
    }
    file: {
      listFiles: (options: {
        pairId?: string
        directory?: string
      }) => Promise<Array<{ path: string; type: 'file' | 'directory' }>>
      parseMentions: (pairId: string, spec: string) => Promise<string>
      readContent: (options: {
        pairId?: string
        directory?: string
        filePath: string
      }) => Promise<string>
    }
    repo: {
      getFileDiff: (directory: string, filePath: string, status: string) => Promise<string>
      commitChanges: (
        directory: string,
        message: string
      ) => Promise<{ sha: string; filesCommitted: number }>
      pushChanges: (
        directory: string
      ) => Promise<{ branch: string; remote: string; upToDate: boolean }>
    }
    skill: {
      discover: (
        projectDir?: string
      ) => Promise<Array<{ name: string; description: string; source: string }>>
      readContent: (name: string, projectDir?: string) => Promise<string>
      refresh: (
        projectDir?: string
      ) => Promise<Array<{ name: string; description: string; source: string }>>
    }
    insights: {
      recordIntervention: (input: {
        pairId: string
        kind: string
        outcome?: string
      }) => Promise<void>
    }
  }
}

declare const __APP_VERSION__: string
