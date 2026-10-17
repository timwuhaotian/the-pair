import { expect, test } from '@playwright/test'
import { openApp } from '../fixtures/app.ts'

const idleActivity = (label: string) => ({
  phase: 'idle',
  label,
  startedAt: 1_000,
  updatedAt: 1_000,
  outputLineCount: 0
})

const snapshot = {
  pairId: 'pair-search-e2e',
  name: 'Search Pair',
  directory: '/repo/.worktrees/pair-search-e2e',
  spec: 'change things',
  status: 'Finished',
  iterations: 1,
  maxIterations: 0,
  turn: 'mentor',
  mentorModel: 'grok:fast',
  executorModel: 'grok:fast',
  messages: [],
  mentorActivity: idleActivity('Mentor idle'),
  executorActivity: idleActivity('Executor idle'),
  mentorCpu: 0,
  mentorMemMb: 0,
  executorCpu: 0,
  executorMemMb: 0,
  cpuUsage: 0,
  memUsage: 0,
  modifiedFiles: [],
  gitTracking: { available: true },
  automationMode: 'full-auto',
  runCount: 1,
  runHistory: [],
  currentRunStartedAt: 1_000,
  worktreeBranch: 'the-pair/pair-search-e2e',
  snapshotVersion: 1,
  savedAt: 1_000,
  providerSessions: {}
}

/** Records the app's preventDefault decision for real 'f' keydowns. */
async function armDefaultProbe(page: import('@playwright/test').Page): Promise<void> {
  await page.evaluate(() => {
    const w = window as unknown as { __lastFPrevented?: boolean }
    window.addEventListener('keydown', (e) => {
      // Modifier keydowns (Meta/Ctrl themselves) would overwrite the reading;
      // only the final 'f' press carries the app's decision.
      if (e.key.toLowerCase() === 'f') w.__lastFPrevented = e.defaultPrevented
    })
  })
}

async function lastDefaultPrevented(page: import('@playwright/test').Page): Promise<boolean> {
  return page.evaluate(
    () => (window as unknown as { __lastFPrevented?: boolean }).__lastFPrevented ?? false
  )
}

test.describe('Transcript search shortcut (headless)', () => {
  test('Cmd/Ctrl+F opens search on the exact platform modifier only', async ({ page }) => {
    await openApp(page, { snapshots: [snapshot] })
    await page.getByText('Search Pair').first().click()
    await expect(page.getByTestId('console-search-toggle')).toBeVisible()

    const isMac = process.platform === 'darwin'
    const combo = isMac ? 'Meta+f' : 'Control+f'
    const wrongCombos = isMac
      ? ['Control+f', 'Meta+Control+f', 'Meta+Shift+f', 'Meta+Alt+f']
      : ['Meta+f', 'Control+Shift+f', 'Control+Alt+f', 'Control+Meta+f']

    // Wrong modifier sets neither open search nor swallow the default (on
    // macOS Ctrl+F is the caret-forward binding, Cmd+Ctrl+F fullscreen).
    for (const wrong of wrongCombos) {
      await armDefaultProbe(page)
      await page.keyboard.press(wrong)
      await expect(page.getByTestId('console-search-input')).toHaveCount(0)
      expect(await lastDefaultPrevented(page)).toBe(false)
    }

    // The exact platform modifier opens — and claims — the search.
    await armDefaultProbe(page)
    await page.keyboard.press(combo)
    await expect(page.getByTestId('console-search-input')).toBeVisible()
    await expect(page.getByTestId('console-search-input')).toBeFocused()
    expect(await lastDefaultPrevented(page)).toBe(true)

    // Re-pressing refocuses the input after the reader clicked away.
    await page.getByTestId('console-search-toggle').click()
    await page.keyboard.press(combo)
    await expect(page.getByTestId('console-search-input')).toBeFocused()
  })

  test('the shortcuts sheet lists the search shortcut', async ({ page }) => {
    await openApp(page, { snapshots: [snapshot] })
    await page.getByText('Search Pair').first().click()
    await expect(page.getByTestId('console-search-toggle')).toBeVisible()

    await page.keyboard.press('Shift+Slash')
    await expect(page.getByText('Search Transcript')).toBeVisible()
  })

  test('Cmd/Ctrl+F stays inside an open modal dialog instead of stealing focus', async ({
    page
  }) => {
    await openApp(page, { snapshots: [snapshot] })
    await page.getByText('Search Pair').first().click()
    await expect(page.getByTestId('console-search-toggle')).toBeVisible()

    const isMac = process.platform === 'darwin'
    const combo = isMac ? 'Meta+f' : 'Control+f'

    // The palette is an aria-modal dialog with its own input.
    await page.keyboard.press(isMac ? 'Meta+k' : 'Control+k')
    const paletteInput = page.getByTestId('command-palette-input')
    await expect(paletteInput).toBeVisible()
    await expect(paletteInput).toBeFocused()

    await armDefaultProbe(page)
    await page.keyboard.press(combo)
    await expect(page.getByTestId('console-search-input')).toHaveCount(0)
    await expect(paletteInput).toBeFocused()
    expect(await lastDefaultPrevented(page)).toBe(false)

    // Same for the shortcuts sheet: the console search never opens behind it.
    await page.keyboard.press('Escape')
    await page.keyboard.press('Shift+Slash')
    await expect(page.getByText('Search Transcript')).toBeVisible()
    await page.keyboard.press(combo)
    await expect(page.getByTestId('console-search-input')).toHaveCount(0)

    // With no dialog open the shortcut works again.
    await page.keyboard.press('Escape')
    await page.keyboard.press(combo)
    await expect(page.getByTestId('console-search-input')).toBeVisible()
  })
})
