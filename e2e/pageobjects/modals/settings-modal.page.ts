import { S, modalTitle } from '../../helpers/selectors.js'
import { BasePage } from '../base.page.js'

export class SettingsModalPage extends BasePage {
  async waitForOpen(): Promise<void> {
    await this.waitForModal('Pair Defaults')
  }

  async save(): Promise<void> {
    await this.click(S.SAVE_DEFAULTS_BTN)
  }

  async cancel(): Promise<void> {
    await this.click(S.CANCEL_BTN_SETTINGS)
  }

  async waitForClosed(timeout = 5000): Promise<void> {
    await $(modalTitle('Pair Defaults')).waitForDisplayed({
      timeout,
      reverse: true
    })
  }
}
