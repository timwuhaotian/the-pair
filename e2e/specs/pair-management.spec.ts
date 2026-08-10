import { DashboardPage } from '../pageobjects/dashboard.page.js'
import { CreatePairModalPage } from '../pageobjects/modals/create-pair-modal.page.js'
import { AssignTaskModalPage } from '../pageobjects/modals/assign-task-modal.page.js'
import { PairDetailPage } from '../pageobjects/pair-detail.page.js'
import { ConfirmModalPage } from '../pageobjects/modals/confirm-modal.page.js'

const dashboard = new DashboardPage()
const createModal = new CreatePairModalPage()
const assignModal = new AssignTaskModalPage()
const pairDetail = new PairDetailPage()
const confirmModal = new ConfirmModalPage()

const TEST_DIR = process.env.E2E_TEST_DIR || '/tmp/e2e-the-pair-test'

describe('Pair Management - multiple pairs and deletion', () => {
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

  it('should create two pairs, delete one, and keep the other', async () => {
    const keepName = 'Keep Pair'
    const deleteName = 'Delete Pair'

    // Create first pair.
    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.setName(keepName)
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('Keep me')
    await createModal.submit()
    await createModal.waitForClosed()

    // Create second pair.
    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.setName(deleteName)
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('Delete me')
    await createModal.submit()
    await createModal.waitForClosed()

    // Both cards should be visible.
    await dashboard.waitForPairCard(keepName)
    await dashboard.waitForPairCard(deleteName)

    // Click the delete button on the card whose text includes deleteName.
    // The delete button uses data-testid="pair-card-delete-{id}" but we don't
    // know the id at test time, so we locate it via the pair name in the DOM.
    await browser.execute((targetName: string) => {
      const buttons = document.querySelectorAll('[data-testid^="pair-card-delete-"]')
      for (const btn of buttons) {
        const card = btn.closest('button')
        if (card && card.textContent && card.textContent.includes(targetName)) {
          ;(btn as HTMLElement).click()
          return
        }
      }
    }, deleteName)

    // Confirm modal appears.
    await confirmModal.waitForOpen()
    await confirmModal.confirm()
    await confirmModal.waitForClosed()

    // Deleted pair card should be gone; kept pair should still be visible.
    await browser.waitUntil(async () => !(await dashboard.isPairCardVisible(deleteName)), {
      timeout: 5000,
      timeoutMsg: 'Deleted pair card still visible'
    })
    const keepVisible = await dashboard.isPairCardVisible(keepName)
    expect(keepVisible).toBe(true)
  })
})

describe('Pair Management - clear session', () => {
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

  it('should clear the console after a finished run', async () => {
    const name = 'Clear Session Pair'
    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.setName(name)
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('Clear session test')
    await createModal.submit()
    await createModal.waitForClosed()

    await dashboard.clickPairCard(name)
    await assignModal.waitForOpen(name)
    await assignModal.submit()
    await assignModal.waitForClosed()

    await pairDetail.waitForStatus('Finished', 20000)
    await pairDetail.waitForConsoleMessage('Plan', 5000)

    // The clear-session button should now be enabled (pair finished + has messages).
    await browser.waitUntil(async () => !(await dashboard.isClearSessionButtonDisabled()), {
      timeout: 5000,
      timeoutMsg: 'Clear Session button was never enabled'
    })

    await dashboard.clickClearSession()
    await confirmModal.waitForOpen()
    await confirmModal.confirm()
    await confirmModal.waitForClosed()

    // After clearing, the console panel should have no messages.
    await browser.waitUntil(
      async () => {
        const text = await pairDetail.getConsoleText()
        return !text.includes('Plan') && !text.includes('Done')
      },
      { timeout: 5000, timeoutMsg: 'Console was not cleared' }
    )
  })
})
