import { S } from '../helpers/selectors.js'
import { BasePage } from './base.page.js'

export class PairDetailPage extends BasePage {
  // ── Lifecycle controls ────────────────────────────────────────────
  async clickPause(): Promise<void> {
    await this.click(S.PAUSE_BTN)
  }

  async clickResume(): Promise<void> {
    await this.click(S.RESUME_BTN)
  }

  async clickRetryTurn(): Promise<void> {
    await this.click(S.RETRY_BTN)
  }

  // ── Status helpers ────────────────────────────────────────────────
  async waitForStatus(status: string, timeout = 15000): Promise<void> {
    await this.waitForText(S.STATUS(status), status, timeout)
  }

  async getStatusText(): Promise<string> {
    return await this.getText('span[class*="rounded-full"]')
  }

  // ── Button visibility ─────────────────────────────────────────────
  async isPauseButtonVisible(): Promise<boolean> {
    return await this.isDisplayed(S.PAUSE_BTN)
  }

  async isResumeButtonVisible(): Promise<boolean> {
    return await this.isDisplayed(S.RESUME_BTN)
  }

  async isRetryButtonVisible(): Promise<boolean> {
    return await this.isDisplayed(S.RETRY_BTN)
  }

  // ── Console messages ──────────────────────────────────────────────
  async getConsoleMessages(): Promise<string[]> {
    const elements = await $$(S.CONSOLE_PANEL + ' div')
    return Promise.all(elements.map((el) => el.getText()))
  }

  async waitForConsoleMessage(text: string, timeout = 10000): Promise<void> {
    await browser.waitUntil(
      async () => {
        const messages = await this.getConsoleMessages()
        return messages.some((m) => m.includes(text))
      },
      { timeout, timeoutMsg: `Expected console message containing "${text}"` }
    )
  }

  async getConsoleText(): Promise<string> {
    return await this.getText(S.CONSOLE_PANEL)
  }

  // ── Message filter bar ────────────────────────────────────────────
  async clickFilterAll(): Promise<void> {
    await this.click(S.FILTER_ALL)
  }

  async clickFilterMentor(): Promise<void> {
    await this.click(S.FILTER_MENTOR)
  }

  async clickFilterExecutor(): Promise<void> {
    await this.click(S.FILTER_EXECUTOR)
  }

  async getFilterCount(filter: 'all' | 'mentor' | 'executor'): Promise<string> {
    const sel =
      filter === 'all' ? S.FILTER_ALL : filter === 'mentor' ? S.FILTER_MENTOR : S.FILTER_EXECUTOR
    return await this.getText(sel)
  }

  // ── Plan review ───────────────────────────────────────────────────
  async clickPlanApprove(): Promise<void> {
    await this.click(S.PLAN_APPROVE_BTN)
  }

  async clickPlanReject(): Promise<void> {
    await this.click(S.PLAN_REJECT_BTN)
  }

  async clickPlanSendBack(): Promise<void> {
    await this.click(S.PLAN_SENDBACK_BTN)
  }

  async setPlanFeedback(text: string): Promise<void> {
    await this.setValue(S.PLAN_FEEDBACK_INPUT, text)
  }

  async isPlanApproveVisible(): Promise<boolean> {
    return await this.isDisplayed(S.PLAN_APPROVE_BTN)
  }

  // ── Console actions ───────────────────────────────────────────────
  async clickStopTurn(): Promise<void> {
    await this.click(S.CONSOLE_STOP_TURN_BTN)
  }

  async clickClearConsole(): Promise<void> {
    await this.click(S.CONSOLE_CLEAR_BTN)
  }

  async isStopTurnVisible(): Promise<boolean> {
    return await this.isDisplayed(S.CONSOLE_STOP_TURN_BTN)
  }

  // ── Inline task input ─────────────────────────────────────────────
  async setInlineTask(text: string): Promise<void> {
    const el = $(S.CONSOLE_TASK_TEXTAREA)
    await el.waitForDisplayed({ timeout: 5000 })
    await el.setValue(text)
  }

  async submitInlineTask(): Promise<void> {
    await browser.keys(['Enter'])
  }

  // ── Error panel ───────────────────────────────────────────────────
  async isErrorPanelVisible(): Promise<boolean> {
    return await this.isDisplayed(S.ERROR_PANEL)
  }

  async clickErrorRetry(): Promise<void> {
    await this.click(S.ERROR_RETRY_BTN)
  }

  // ── Iteration progress ────────────────────────────────────────────
  async isIterationProgressVisible(): Promise<boolean> {
    return await this.isDisplayed(S.ITERATION_PROGRESS)
  }

  async getIterationProgressText(): Promise<string> {
    return await this.getText(S.ITERATION_PROGRESS)
  }
}
