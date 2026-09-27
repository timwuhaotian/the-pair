import { execSync } from 'node:child_process'
import { existsSync, readdirSync } from 'node:fs'
import { strictEqual } from 'node:assert/strict'
import { DashboardPage } from '../pageobjects/dashboard.page.js'
import { CreatePairModalPage } from '../pageobjects/modals/create-pair-modal.page.js'
import { AssignTaskModalPage } from '../pageobjects/modals/assign-task-modal.page.js'
import { PairDetailPage } from '../pageobjects/pair-detail.page.js'
import { S } from '../helpers/selectors.js'

const dashboard = new DashboardPage()
const createModal = new CreatePairModalPage()
const assignModal = new AssignTaskModalPage()
const pairDetail = new PairDetailPage()

const TEST_DIR = process.env.E2E_TEST_DIR || '/tmp/e2e-the-pair-worktree-test'
const PAIR_NAME = 'Worktree Pair'

function git(args: string): string {
  return execSync(`git ${args}`, { cwd: TEST_DIR, stdio: 'pipe' }).toString().trim()
}

function pairWorktrees(): string[] {
  const dir = `${TEST_DIR}/.worktrees`
  if (!existsSync(dir)) return []
  return readdirSync(dir)
    .filter((name) => name.startsWith('pair-'))
    .sort()
}

function pairBranches(): string[] {
  return git("branch --list 'the-pair/*' --format='%(refname:short)'")
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .sort()
}

describe('Worktree isolation', () => {
  before(async () => {
    await browser.call(async () => {
      const { mkdirSync, writeFileSync, rmSync } = await import('node:fs')
      const { execSync } = await import('node:child_process')
      rmSync(TEST_DIR, { recursive: true, force: true })
      mkdirSync(TEST_DIR, { recursive: true })
      // Unlike the other specs' bare `git init`, this repo has a commit so the
      // create modal's worktree default applies (unborn repos stay in-place).
      execSync(`cd ${TEST_DIR} && git init -q`, { stdio: 'pipe' })
      execSync(
        `cd ${TEST_DIR} && git config user.name Test && git config user.email test@example.com`,
        { stdio: 'pipe' }
      )
      writeFileSync(`${TEST_DIR}/README.md`, 'hello\n')
      execSync(`cd ${TEST_DIR} && git add README.md && git commit -q -m init`, {
        stdio: 'pipe'
      })
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

  it('defaults to a fresh worktree in a git repo and runs the pair inside it', async () => {
    await dashboard.clickNewPair()
    await createModal.waitForOpen()
    await createModal.setName(PAIR_NAME)
    await createModal.setDirectory(TEST_DIR)
    // The directory input commits its scan on blur — refocus the name field.
    await $(S.NAME_INPUT).click()

    // A committed git repo defaults the toggle on.
    await $(S.WORKTREE_TOGGLE).waitForDisplayed({ timeout: 10000 })
    await browser.waitUntil(
      async () => (await $(S.WORKTREE_TOGGLE).getAttribute('aria-pressed')) === 'true',
      { timeoutMsg: 'worktree toggle should default on for a git repo' }
    )

    await createModal.setTaskSpec('Worktree isolation test')
    await createModal.submit()
    await createModal.waitForClosed()

    const worktrees = await browser.call(async () => pairWorktrees())
    strictEqual(worktrees.length, 1, `expected one pair worktree, got ${worktrees}`)
    const branches = await browser.call(async () => pairBranches())
    strictEqual(branches.length, 1, `expected one the-pair/* branch, got ${branches}`)
    strictEqual(branches[0], `the-pair/${worktrees[0]}`)

    await dashboard.isPairCardVisible(PAIR_NAME)
  })

  it('rotates to a fresh worktree for the next task', async () => {
    // The mock run from creation finishes fast; the card then opens the pair
    // view with its assign modal (same navigation the execution spec relies on).
    await dashboard.clickPairCard(PAIR_NAME)
    await assignModal.waitForOpen(PAIR_NAME)

    await $(S.FRESH_WORKTREE_TOGGLE).waitForDisplayed({ timeout: 10000 })
    await $(S.FRESH_WORKTREE_TOGGLE).click()

    await assignModal.setTaskSpec('Second isolated task')
    await assignModal.submit()
    await assignModal.waitForClosed()

    // The rotated pair runs in the only worktree directory, and its branch is
    // the one live the-pair/* branch: the old run made no commits, so its
    // branch was pruned as redundant instead of being kept around.
    const worktrees = await browser.call(async () => pairWorktrees())
    strictEqual(worktrees.length, 1, `expected one pair worktree after rotation, got ${worktrees}`)
    const branches = await browser.call(async () => pairBranches())
    strictEqual(
      branches.includes(`the-pair/${worktrees[0]}`),
      true,
      `new worktree branch missing from ${branches}`
    )

    // The branch chip in the chrome shows where the pair works.
    await $(S.BRANCH_CHIP).waitForDisplayed({ timeout: 10000 })
    const chip = await $(S.BRANCH_CHIP).getText()
    strictEqual(chip.includes(`the-pair/${worktrees[0]}`), true, `chip was ${chip}`)

    await pairDetail.waitForStatus('Finished', 20000)
  })
})
