import { S } from '../../helpers/selectors.js'
import { BasePage } from '../base.page.js'

export class ConfirmModalPage extends BasePage {
  async waitForOpen(title?: string, timeout = 5000): Promise<void> {
    if (title) {
      await this.waitForDisplayed(`h2*=${title}`, timeout)
    } else {
      await this.waitForDisplayed(S.CONFIRM_MODAL_CONFIRM, timeout)
    }
  }

  async confirm(): Promise<void> {
    await this.click(S.CONFIRM_MODAL_CONFIRM)
  }

  async cancel(): Promise<void> {
    await this.click(S.CONFIRM_MODAL_CANCEL)
  }

  async waitForClosed(timeout = 5000): Promise<void> {
    await $(S.CONFIRM_MODAL_CONFIRM).waitForDisplayed({ timeout, reverse: true })
  }
}
