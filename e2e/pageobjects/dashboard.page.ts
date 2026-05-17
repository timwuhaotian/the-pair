import { S } from '../helpers/selectors.js'
import { BasePage } from './base.page.js'

export class DashboardPage extends BasePage {
  async clickNewPair(): Promise<void> {
    await this.click(S.NEW_PAIR_BTN)
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

  async clickPairCard(name: string): Promise<void> {
    await this.click(S.PAIR_CARD(name))
  }

  async clickBack(): Promise<void> {
    await this.click(S.BACK_BTN)
  }

  async isPairCardVisible(name: string): Promise<boolean> {
    return await this.isDisplayed(S.PAIR_CARD(name))
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
}
