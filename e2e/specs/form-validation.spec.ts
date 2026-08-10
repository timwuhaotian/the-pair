import { DashboardPage } from '../pageobjects/dashboard.page.js'
import { CreatePairModalPage } from '../pageobjects/modals/create-pair-modal.page.js'
import { S } from '../helpers/selectors.js'

const dashboard = new DashboardPage()
const createModal = new CreatePairModalPage()

const TEST_DIR = process.env.E2E_TEST_DIR || '/tmp/e2e-the-pair-test'

describe('Form Validation', () => {
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

  it('should create a pair when all fields are filled', async () => {
    const name = 'Form Valid All Fields'
    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.setName(name)
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('Do all the things')
    await createModal.submit()
    await createModal.waitForClosed()

    const visible = await dashboard.isPairCardVisible(name)
    expect(visible).toBe(true)
  })

  it('should have a disabled submit button when the name is empty', async () => {
    await dashboard.clickNewPair()
    await createModal.waitForOpen()

    // Fill directory + spec but leave name empty.
    await createModal.setDirectory(TEST_DIR)
    await createModal.setTaskSpec('No name provided')

    const disabled = await createModal.isButtonDisabled(S.CREATE_PAIR_SUBMIT)
    expect(disabled).toBe(true)

    await createModal.cancel()
    await createModal.waitForClosed()
  })

  it('should not submit when task spec is empty (required field)', async () => {
    await dashboard.clickNewPair()
    await createModal.waitForOpen()

    await createModal.setName('Form Missing Spec')
    await createModal.setDirectory(TEST_DIR)
    // Intentionally leave task spec blank.

    // The submit button should be disabled because the spec textarea is required.
    const disabled = await createModal.isButtonDisabled(S.CREATE_PAIR_SUBMIT)
    expect(disabled).toBe(true)

    await createModal.cancel()
    await createModal.waitForClosed()
  })

  it('should close the create pair modal on Escape', async () => {
    await dashboard.clickNewPair()
    await createModal.waitForOpen()

    await browser.keys(['Escape'])
    await createModal.waitForClosed()
  })
})
