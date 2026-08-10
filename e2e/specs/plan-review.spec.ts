import { DashboardPage } from '../pageobjects/dashboard.page.js'
import { CreatePairModalPage } from '../pageobjects/modals/create-pair-modal.page.js'
import { AssignTaskModalPage } from '../pageobjects/modals/assign-task-modal.page.js'
import { PairDetailPage } from '../pageobjects/pair-detail.page.js'

const dashboard = new DashboardPage()
const createModal = new CreatePairModalPage()
const assignModal = new AssignTaskModalPage()
const pairDetail = new PairDetailPage()

const TEST_DIR = process.env.E2E_TEST_DIR || '/tmp/e2e-the-pair-test'

describe('Plan Review - plan gate approval flow', () => {
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

  it('should pause for human review when plan gate is enabled, then finish after approval', async () => {
    const name = 'Plan Gate Pair'
    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.setName(name)
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('Plan gate review test')
    // Enable the plan gate — the mentor's first plan will pause for review.
    await createModal.togglePlanGate()
    await createModal.submit()
    await createModal.waitForClosed()

    await dashboard.clickPairCard(name)
    await assignModal.waitForOpen(name)
    await assignModal.submit()
    await assignModal.waitForClosed()

    // With the plan gate enabled the pair should reach "Awaiting Human Review"
    // after the mentor delivers the initial plan.
    await pairDetail.waitForStatus('Awaiting Human Review', 25000)

    // The plan-approve button must be visible.
    const approveVisible = await pairDetail.isPlanApproveVisible()
    expect(approveVisible).toBe(true)

    // Approve the plan — the pair continues to execution and finishes.
    await pairDetail.clickPlanApprove()

    await pairDetail.waitForStatus('Finished', 20000)
    await pairDetail.waitForConsoleMessage('Plan', 5000)
    await pairDetail.waitForConsoleMessage('Done', 5000)
  })
})
