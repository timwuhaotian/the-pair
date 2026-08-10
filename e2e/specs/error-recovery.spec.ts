import { DashboardPage } from '../pageobjects/dashboard.page.js'
import { CreatePairModalPage } from '../pageobjects/modals/create-pair-modal.page.js'
import { AssignTaskModalPage } from '../pageobjects/modals/assign-task-modal.page.js'
import { PairDetailPage } from '../pageobjects/pair-detail.page.js'

const dashboard = new DashboardPage()
const createModal = new CreatePairModalPage()
const assignModal = new AssignTaskModalPage()
const pairDetail = new PairDetailPage()

const TEST_DIR = process.env.E2E_TEST_DIR || '/tmp/e2e-the-pair-test'

/**
 * Error → retry → success flow.
 *
 * The error-scenario mock makes the executor fail with "Permission denied".
 * After the pair enters Error status we reset the scenario to 'success' so
 * that clicking retry re-runs the turn against a healthy mock.
 */
describe('Error Recovery - error then retry succeeds', () => {
  before(async () => {
    await browser.call(async () => {
      const { mkdirSync } = await import('node:fs')
      const { execSync } = await import('node:child_process')
      mkdirSync(TEST_DIR, { recursive: true })
      execSync(`cd ${TEST_DIR} && git init`, { stdio: 'pipe' })
    })
    process.env.THE_PAIR_E2E_MOCK_SCENARIO = 'error'
  })

  after(async () => {
    process.env.THE_PAIR_E2E_MOCK_SCENARIO = 'success'
    await browser.call(async () => {
      const { rmSync } = await import('node:fs')
      rmSync(TEST_DIR, { recursive: true, force: true })
    })
  })

  it('should show Error status, then recover after retry', async () => {
    const name = 'Error Recovery Pair'
    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.setName(name)
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('Trigger an error then recover')
    await createModal.submit()
    await createModal.waitForClosed()

    await dashboard.clickPairCard(name)
    await assignModal.waitForOpen(name)
    await assignModal.submit()
    await assignModal.waitForClosed()

    // The mock executor returns "Error: Permission denied" → Error status.
    await pairDetail.waitForStatus('Error', 20000)

    // The error panel must be visible.
    const errorVisible = await pairDetail.isErrorPanelVisible()
    expect(errorVisible).toBe(true)

    // Switch to the success scenario BEFORE retrying so the retried turn
    // runs against a healthy mock.
    process.env.THE_PAIR_E2E_MOCK_SCENARIO = 'success'

    await pairDetail.clickErrorRetry()

    // After retry the pair should reach Finished.
    await pairDetail.waitForStatus('Finished', 20000)
  })
})
