import { expect, test } from '@playwright/test'
import { openApp } from '../fixtures/app.ts'

test.describe('App shell navigation (headless)', () => {
  test('rail switches between pairs, activity, models, and insights sections', async ({ page }) => {
    await openApp(page)

    const rail = page.getByTestId('app-shell-rail')
    await expect(rail).toBeVisible()

    // Default section is the pairs hub.
    await expect(page.getByTestId('pairs-hub-empty')).toBeVisible()
    await expect(page.getByRole('button', { name: /create your first pair/i })).toBeVisible()

    await page.getByTestId('shell-nav-activity').click()
    await expect(page.getByTestId('activity-view')).toBeVisible()
    await expect(page.getByText('Activity will appear here when Pairs')).toBeVisible()

    await page.getByTestId('shell-nav-models').click()
    await expect(page.getByTestId('models-view')).toBeVisible()
    await expect(page.getByTestId('provider-card-anthropic-1')).toBeVisible()

    await page.getByTestId('shell-nav-insights').click()
    await expect(page.getByTestId('insights-view')).toBeVisible()

    await page.getByTestId('shell-nav-pairs').click()
    await expect(page.getByTestId('pairs-hub-empty')).toBeVisible()
  })
})
