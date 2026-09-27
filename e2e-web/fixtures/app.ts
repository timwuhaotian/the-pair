import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import { Page } from '@playwright/test'

const here = dirname(fileURLToPath(import.meta.url))
export const MOCK_INIT_PATH = join(here, '..', 'mock-init.js')

export interface MockState {
  repo?: {
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
  nonGitDir?: string
  worktree?: {
    pairId: string
    directory: string
    branch: string | null
    repoPath: string
    worktreePath: string
    worktreeBranch: string
  }
}

/** Loads the app with the Tauri IPC layer mocked and the given state enabled. */
export async function openApp(page: Page, state: MockState = {}): Promise<void> {
  // Seed first: mock-init.js reads window.__MOCK_STATE__ when it runs.
  await page.addInitScript((seed) => {
    ;(window as unknown as { __MOCK_STATE__: unknown }).__MOCK_STATE__ = seed
  }, state)
  await page.addInitScript({ path: MOCK_INIT_PATH })
  await page.goto('/')
}

/** Commands the UI sent to the (mocked) backend, in order. */
export async function mockCalls(page: Page): Promise<Array<{ cmd: string; args: unknown }>> {
  return page.evaluate(
    () =>
      (window as unknown as { __MOCK_CALLS__: Array<{ cmd: string; args: unknown }> })
        .__MOCK_CALLS__
  )
}
