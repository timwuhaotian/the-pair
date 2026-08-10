import { DashboardPage } from '../pageobjects/dashboard.page.js'
import { CreatePairModalPage } from '../pageobjects/modals/create-pair-modal.page.js'
import { AssignTaskModalPage } from '../pageobjects/modals/assign-task-modal.page.js'
import { PairDetailPage } from '../pageobjects/pair-detail.page.js'

const dashboard = new DashboardPage()
const createModal = new CreatePairModalPage()
const assignModal = new AssignTaskModalPage()
const pairDetail = new PairDetailPage()

const TEST_DIR = process.env.E2E_TEST_DIR || '/tmp/e2e-the-pair-test'

describe('Console Features - messages, filters, iteration progress', () => {
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

  it('should show iteration progress, mentor and executor messages, and filter correctly', async () => {
    const name = 'Console Features Pair'
    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.setName(name)
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('Console features test')
    await createModal.submit()
    await createModal.waitForClosed()

    await dashboard.clickPairCard(name)
    await assignModal.waitForOpen(name)
    await assignModal.submit()
    await assignModal.waitForClosed()

    await pairDetail.waitForStatus('Finished', 20000)

    // Iteration progress should be visible after a run.
    const progressVisible = await pairDetail.isIterationProgressVisible()
    expect(progressVisible).toBe(true)

    // Console should contain mentor (Plan) and executor (Done) messages.
    await pairDetail.waitForConsoleMessage('Plan', 5000)
    await pairDetail.waitForConsoleMessage('Done', 5000)

    // Filter to Mentor only — Done messages should disappear.
    await pairDetail.clickFilterMentor()
    await browser.waitUntil(async () => !(await pairDetail.getConsoleText()).includes('Done'), {
      timeout: 5000,
      timeoutMsg: 'Executor messages still visible under Mentor filter'
    })
    const mentorText = await pairDetail.getConsoleText()
    expect(mentorText.includes('Plan')).toBe(true)

    // Filter to Executor only — Plan messages should disappear.
    await pairDetail.clickFilterExecutor()
    await browser.waitUntil(async () => !(await pairDetail.getConsoleText()).includes('Plan'), {
      timeout: 5000,
      timeoutMsg: 'Mentor messages still visible under Executor filter'
    })
    const executorText = await pairDetail.getConsoleText()
    expect(executorText.includes('Done')).toBe(true)

    // Back to All — both should be visible again.
    await pairDetail.clickFilterAll()
    await pairDetail.waitForConsoleMessage('Plan', 5000)
    await pairDetail.waitForConsoleMessage('Done', 5000)
  })
})
