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
 * Verify that different provider mock models can drive a full pair lifecycle.
 * The mock spawner is provider-agnostic, so any provider-backed model must
 * run the same Mentoring → Executing → Reviewing → Finished lifecycle.
 */
describe('Provider Models - Claude provider', () => {
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

  it('should complete a lifecycle with Claude Sonnet 4 (Mock) on both roles', async () => {
    const name = 'Claude Provider Pair'
    const model = 'Claude Sonnet 4 (Mock)'

    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.setName(name)
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('Verify claude provider end to end')
    await createModal.selectMentorModel(model)
    await createModal.selectExecutorModel(model)
    await createModal.submit()
    await createModal.waitForClosed()

    await dashboard.isPairCardVisible(name)

    await dashboard.clickPairCard(name)
    await assignModal.waitForOpen(name)
    await assignModal.submit()
    await assignModal.waitForClosed()

    await pairDetail.waitForStatus('Finished', 20000)
    await pairDetail.waitForConsoleMessage('Plan', 5000)
    await pairDetail.waitForConsoleMessage('Done', 5000)
  })
})

describe('Provider Models - Pi provider', () => {
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

  it('should complete a lifecycle with Claude Sonnet 4 via Pi (Mock) on both roles', async () => {
    const name = 'Pi Provider Pair'
    const model = 'Claude Sonnet 4 via Pi (Mock)'

    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.setName(name)
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('Verify pi provider end to end')
    await createModal.selectMentorModel(model)
    await createModal.selectExecutorModel(model)
    await createModal.submit()
    await createModal.waitForClosed()

    await dashboard.isPairCardVisible(name)

    await dashboard.clickPairCard(name)
    await assignModal.waitForOpen(name)
    await assignModal.submit()
    await assignModal.waitForClosed()

    await pairDetail.waitForStatus('Finished', 20000)
    await pairDetail.waitForConsoleMessage('Plan', 5000)
    await pairDetail.waitForConsoleMessage('Done', 5000)
  })
})
