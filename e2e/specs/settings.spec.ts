import { DashboardPage } from '../pageobjects/dashboard.page.js'
import { CreatePairModalPage } from '../pageobjects/modals/create-pair-modal.page.js'
import { AssignTaskModalPage } from '../pageobjects/modals/assign-task-modal.page.js'
import { SettingsModalPage } from '../pageobjects/modals/settings-modal.page.js'

const dashboard = new DashboardPage()
const createModal = new CreatePairModalPage()
const assignModal = new AssignTaskModalPage()
const settingsModal = new SettingsModalPage()

const TEST_DIR = process.env.E2E_TEST_DIR || '/tmp/e2e-the-pair-test'

describe('Settings', () => {
  before(async () => {
    await browser.call(async () => {
      const { mkdirSync } = await import('node:fs')
      const { execSync } = await import('node:child_process')
      mkdirSync(TEST_DIR, { recursive: true })
      execSync(`cd ${TEST_DIR} && git init`, { stdio: 'pipe' })
    })
    process.env.THE_PAIR_E2E_MOCK_SCENARIO = 'success'
  })

  after(async () => {
    process.env.THE_PAIR_E2E_MOCK_SCENARIO = 'success'
    await browser.call(async () => {
      const { rmSync } = await import('node:fs')
      rmSync(TEST_DIR, { recursive: true, force: true })
    })
  })

  it('should toggle theme and verify the dark class changes', async () => {
    const before = await dashboard.isDarkTheme()
    await dashboard.clickThemeToggle()
    // Wait for the class to flip.
    await browser.waitUntil(async () => (await dashboard.isDarkTheme()) !== before, {
      timeout: 5000,
      timeoutMsg: 'Theme did not toggle'
    })
    const after = await dashboard.isDarkTheme()
    expect(after).toBe(!before)

    // Toggle back so we don't affect subsequent tests.
    await dashboard.clickThemeToggle()
    await browser.waitUntil(async () => (await dashboard.isDarkTheme()) === before, {
      timeout: 5000,
      timeoutMsg: 'Theme did not toggle back'
    })
  })

  it('should open and close Settings modal from pair detail', async () => {
    // The Models / Settings button only appears when a pair is selected,
    // so create one and enter its detail view first.
    const name = 'Settings Test Pair'
    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.setName(name)
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('Settings modal test')
    await createModal.submit()
    await createModal.waitForClosed()

    await dashboard.clickPairCard(name)
    await assignModal.waitForOpen(name)
    await assignModal.cancel()
    await assignModal.waitForClosed()

    // Now the Models (Settings) button should be visible in the toolbar.
    await dashboard.clickModels()
    await settingsModal.waitForOpen()
    await settingsModal.cancel()
    await settingsModal.waitForClosed()
  })
})
