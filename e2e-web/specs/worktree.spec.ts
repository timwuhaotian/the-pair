import { expect, test } from '@playwright/test'
import { mockCalls, openApp } from '../fixtures/app.ts'

test.describe('Worktree isolation (headless)', () => {
  test('defaults the worktree opt-in on for a committed git repo and sends it', async ({
    page
  }) => {
    await openApp(page)

    await page.getByRole('button', { name: /create your first pair/i }).click()
    await page.getByTestId('pair-name-input').fill('Isolated Pair')
    await page.getByTestId('pair-directory-input').fill('/repo')
    // The directory input commits its git scan on blur.
    await page.getByTestId('pair-name-input').click()

    const toggle = page.getByTestId('worktree-toggle')
    await expect(toggle).toBeVisible()
    await expect(toggle).toHaveAttribute('aria-pressed', 'true')
    // The opt-in shows its preview and the base-branch picker.
    await expect(page.getByText('your current branch stays untouched')).toBeVisible()
    await expect(page.getByText('base branch (defaults to current)')).toBeVisible()

    await page.getByTestId('pair-task-spec').fill('isolated task')
    await page.getByTestId('pair-submit-btn').click()

    const calls = await mockCalls(page)
    const created = calls.find((c) => c.cmd === 'pair_create')
    expect(created, 'pair_create was called').toBeTruthy()
    const input = (created?.args as { input: { useWorktree?: boolean } }).input
    expect(input.useWorktree).toBe(true)

    await expect(page.getByText('Isolated Pair')).toBeVisible()
  })

  test('lets the user opt out and falls back to the plain branch picker', async ({ page }) => {
    await openApp(page)

    await page.getByRole('button', { name: /create your first pair/i }).click()
    await page.getByTestId('pair-directory-input').fill('/repo')
    await page.getByTestId('pair-name-input').click()

    const toggle = page.getByTestId('worktree-toggle')
    await expect(toggle).toHaveAttribute('aria-pressed', 'true')
    await toggle.click()
    await expect(toggle).toHaveAttribute('aria-pressed', 'false')
    await expect(page.getByText('your current branch stays untouched')).toBeHidden()
    await expect(page.getByText('select branch (optional)')).toBeVisible()

    await page.getByTestId('pair-cancel-btn').click()
    const calls = await mockCalls(page)
    expect(calls.find((c) => c.cmd === 'pair_create')).toBeUndefined()
  })

  test('rotates to a fresh worktree for the next task and shows the branch', async ({ page }) => {
    await openApp(page)

    await page.getByRole('button', { name: /create your first pair/i }).click()
    await page.getByTestId('pair-name-input').fill('Rotating Pair')
    await page.getByTestId('pair-directory-input').fill('/repo')
    await page.getByTestId('pair-name-input').click()
    await page.getByTestId('pair-task-spec').fill('first run')
    await page.getByTestId('pair-submit-btn').click()

    // Selecting the pair opens its detail view (chrome + console).
    await page.getByText('Rotating Pair').first().click()
    await expect(page.getByTestId('chrome-branch-chip')).toContainText('the-pair/pair-e2e')

    // The chrome's New Task button opens the assign modal.
    await page.getByTestId('chrome-new-task').click()
    await expect(page.getByRole('heading', { name: /Assign New Task/ })).toBeVisible()

    // The workspace box surfaces the pair's worktree branch.
    await expect(page.getByText('the-pair/pair-e2e').first()).toBeVisible()

    const freshToggle = page.getByTestId('fresh-worktree-toggle')
    await expect(freshToggle).toBeVisible()
    await freshToggle.click()
    await expect(freshToggle).toHaveAttribute('aria-pressed', 'true')

    await page.getByTestId('assign-task-spec').fill('second run on a fresh branch')
    await page.getByTestId('assign-submit-btn').click()

    const calls = await mockCalls(page)
    const assigned = calls.find((c) => c.cmd === 'pair_assign_task')
    expect(assigned, 'pair_assign_task was called').toBeTruthy()
    const input = (assigned?.args as { input: { freshWorktree?: boolean } }).input
    expect(input.freshWorktree).toBe(true)

    // The chrome shows the branch the pair now works on.
    await expect(page.getByTestId('chrome-branch-chip')).toContainText('the-pair/pair-e2e')
  })
})
