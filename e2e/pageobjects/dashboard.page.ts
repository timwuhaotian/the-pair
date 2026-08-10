import { S } from '../helpers/selectors.js'
import { BasePage } from './base.page.js'

export class DashboardPage extends BasePage {
  async clickNewPair(): Promise<void> {
    await this.click(S.NEW_PAIR_BTN)
  }

  async clickNewTask(): Promise<void> {
    await this.click(S.NEW_TASK_BTN)
  }

  async clickClearSession(): Promise<void> {
    await this.click(S.CLEAR_SESSION_BTN)
  }

  async clickModels(): Promise<void> {
    await this.click(S.MODELS_BTN)
  }

  async clickThemeToggle(): Promise<void> {
    await this.click(S.THEME_TOGGLE)
  }

  async clickBack(): Promise<void> {
    await this.click(S.BACK_BTN)
  }

  async clickShortcuts(): Promise<void> {
    await this.click(S.SHORTCUTS_BTN)
  }

  async clickMuteToggle(): Promise<void> {
    await this.click(S.MUTE_TOGGLE)
  }

  async clickLanguageToggle(): Promise<void> {
    await this.click(S.LANGUAGE_TOGGLE)
  }

  async clickPairCard(name: string): Promise<void> {
    await this.click(S.PAIR_CARD(name))
  }

  async isPairCardVisible(name: string): Promise<boolean> {
    return await this.isDisplayed(S.PAIR_CARD(name))
  }

  async waitForPairCard(name: string, timeout = 10000): Promise<void> {
    await this.waitForDisplayed(S.PAIR_CARD(name), timeout)
  }

  async getStatusBadgeText(): Promise<string> {
    return await this.getText('span[class*="rounded-full"]')
  }

  async isToolbarVisible(): Promise<boolean> {
    return await this.isDisplayed(S.TOOLBAR)
  }

  async isClearSessionButtonDisabled(): Promise<boolean> {
    return await this.isButtonDisabled(S.CLEAR_SESSION_BTN)
  }

  async isClearSessionButtonVisible(): Promise<boolean> {
    return await this.isDisplayed(S.CLEAR_SESSION_BTN)
  }

  async isBackButtonVisible(): Promise<boolean> {
    return await this.isDisplayed(S.BACK_BTN)
  }

  async isModelsButtonVisible(): Promise<boolean> {
    return await this.isDisplayed(S.MODELS_BTN)
  }

  async isDarkTheme(): Promise<boolean> {
    const cls = await browser.execute(() => document.documentElement.classList.contains('dark'))
    return cls as boolean
  }
}
