import { DashboardPage } from '../pageobjects/dashboard.page.js'
import { CreatePairModalPage } from '../pageobjects/modals/create-pair-modal.page.js'
import { AssignTaskModalPage } from '../pageobjects/modals/assign-task-modal.page.js'

const dashboard = new DashboardPage()
const createModal = new CreatePairModalPage()
const assignModal = new AssignTaskModalPage()

const TEST_DIR = process.env.E2E_TEST_DIR || '/tmp/e2e-the-pair-test'

describe('Navigation', () => {
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

  it('should show the dashboard toolbar on launch', async () => {
    const visible = await dashboard.isToolbarVisible()
    expect(visible).toBe(true)
  })

  it('should open Create Pair modal from sidebar and close via cancel', async () => {
    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.cancel()
    await createModal.waitForClosed()
  })

  it('should navigate into a pair and back via the back button', async () => {
    const name = 'Nav Back Test'
    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.setName(name)
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('Nav test task')
    await createModal.submit()
    await createModal.waitForClosed()

    await dashboard.waitForPairCard(name)
    await dashboard.clickPairCard(name)
    await assignModal.waitForOpen(name)
    await assignModal.cancel()
    await assignModal.waitForClosed()

    // Inside pair detail the back button should be visible.
    await browser.waitUntil(async () => dashboard.isBackButtonVisible(), {
      timeout: 5000,
      timeoutMsg: 'Back button not visible in pair detail'
    })
    await dashboard.clickBack()

    // Back on the dashboard the pair card should still be visible.
    await dashboard.waitForPairCard(name)
  })

  it('should open and close the Shortcuts modal', async () => {
    await dashboard.clickShortcuts()
    // The Shortcuts modal title is "Keyboard Shortcuts".
    await browser.waitUntil(
      async () => {
        const title = await $('h2=Keyboard Shortcuts')
          .getText()
          .catch(() => '')
        return title.includes('Keyboard Shortcuts')
      },
      { timeout: 5000, timeoutMsg: 'Shortcuts modal did not open' }
    )
    // Close via Escape.
    await browser.keys(['Escape'])
    await $('h2=Keyboard Shortcuts').waitForDisplayed({
      timeout: 5000,
      reverse: true
    })
  })

  it('should toggle theme and verify the dark class changes', async () => {
    const before = await dashboard.isDarkTheme()
    await dashboard.clickThemeToggle()
    await browser.waitUntil(async () => (await dashboard.isDarkTheme()) !== before, {
      timeout: 5000,
      timeoutMsg: 'Theme did not toggle'
    })
    const after = await dashboard.isDarkTheme()
    expect(after).toBe(!before)

    // Toggle back to restore original state.
    await dashboard.clickThemeToggle()
    await browser.waitUntil(async () => (await dashboard.isDarkTheme()) === before, {
      timeout: 5000,
      timeoutMsg: 'Theme did not toggle back'
    })
  })

  it('should exercise the mute toggle without error', async () => {
    // Simply click the mute toggle — we only verify it does not throw.
    await dashboard.clickMuteToggle()
    // Click again to restore original state.
    await dashboard.clickMuteToggle()
  })
})
