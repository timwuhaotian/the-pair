/// <reference types="vite/client" />

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
      getModels: () => Promise<unknown>
      getCachedModels: () => Promise<unknown>
      refreshModels: () => Promise<unknown>
      getProviders: () => Promise<unknown>
      read: () => Promise<unknown>
      openFile: () => Promise<unknown>
      launchLogin: (providerKind: string) => Promise<void>
      getVersion: () => Promise<string>
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
  }
}

declare const __APP_VERSION__: string
