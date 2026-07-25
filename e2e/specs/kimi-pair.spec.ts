import { DashboardPage } from '../pageobjects/dashboard.page.js'
import { CreatePairModalPage } from '../pageobjects/modals/create-pair-modal.page.js'
import { AssignTaskModalPage } from '../pageobjects/modals/assign-task-modal.page.js'
import { PairDetailPage } from '../pageobjects/pair-detail.page.js'

const dashboard = new DashboardPage()
const createModal = new CreatePairModalPage()
const assignModal = new AssignTaskModalPage()
const pairDetail = new PairDetailPage()

const TEST_DIR = process.env.E2E_TEST_DIR || '/tmp/e2e-the-pair-test'
const PAIR_NAME = 'Kimi KAT Pair'
// Display name of the Kimi mock model seeded by `detect_all_mock()`
// (model id `wanqing-streamlake/kat-coder-pro-v2.5`, provider `kimi`).
const KAT_MODEL = 'KAT Coder Pro (Mock)'

describe('Kimi Provider - KAT pair lifecycle', () => {
  before(async () => {
    await browser.call(async () => {
      const { mkdirSync } = await import('node:fs')
      mkdirSync(TEST_DIR, { recursive: true })
      const { execSync } = await import('node:child_process')
      execSync(`cd ${TEST_DIR} && git init`, { stdio: 'pipe' })
    })
    process.env.THE_PAIR_E2E_MOCK_SCENARIO = 'success'
  })

  after(async () => {
    await browser.call(async () => {
      const { rmSync } = await import('node:fs')
      rmSync(TEST_DIR, { recursive: true, force: true })
    })
  })

  it('should create a pair with the Kimi KAT model on both roles and finish a run', async () => {
    await dashboard.clickNewPair()
    await createModal.waitForOpen()

    await createModal.setName(PAIR_NAME)
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('Verify the kimi provider end to end')
    await createModal.selectMentorModel(KAT_MODEL)
    await createModal.selectExecutorModel(KAT_MODEL)

    await createModal.submit()
    await createModal.waitForClosed()

    await dashboard.isPairCardVisible(PAIR_NAME)

    await dashboard.clickPairCard(PAIR_NAME)
    await assignModal.waitForOpen(PAIR_NAME)
    await assignModal.submit()
    await assignModal.waitForClosed()

    // The mock spawner is provider-agnostic, so a kimi-backed pair must run
    // the same Mentoring -> Executing -> Reviewing -> Finished lifecycle.
    await pairDetail.waitForStatus('Finished', 20000)
    await pairDetail.waitForConsoleMessage('Plan', 5000)
    await pairDetail.waitForConsoleMessage('Done', 5000)
  })
})
