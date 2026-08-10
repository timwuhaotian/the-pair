import { S, modalTitle } from '../../helpers/selectors.js'
import { BasePage } from '../base.page.js'

export class CreatePairModalPage extends BasePage {
  async waitForOpen(): Promise<void> {
    await this.waitForModal('Create New Pair')
  }

  async setName(name: string): Promise<void> {
    await this.setValue(S.NAME_INPUT, name)
  }

  async setDirectory(path: string): Promise<void> {
    await this.setValue(S.DIRECTORY_INPUT, path)
  }

  async setTaskSpec(spec: string): Promise<void> {
    await this.setValue(S.TASK_SPEC_CREATE, spec)
  }

  async togglePlanGate(): Promise<void> {
    await this.click(S.PLAN_GATE_TOGGLE)
  }

  async selectMentorModel(modelName: string): Promise<void> {
    await this._selectModelInPicker('Mentor', modelName)
  }

  async selectExecutorModel(modelName: string): Promise<void> {
    await this._selectModelInPicker('Executor', modelName)
  }

  async submit(): Promise<void> {
    await this.click(S.CREATE_PAIR_SUBMIT)
  }

  async cancel(): Promise<void> {
    await this.click(S.CANCEL_BTN_CREATE)
  }

  async waitForClosed(timeout = 5000): Promise<void> {
    await $(modalTitle('Create New Pair')).waitForDisplayed({
      timeout,
      reverse: true
    })
  }

  async isErrorVisible(): Promise<boolean> {
    return await this.isDisplayed(S.ERROR_PANEL)
  }

  private async _selectModelInPicker(role: string, modelName: string): Promise<void> {
    const roleLabel = role === 'Mentor' ? S.MENTOR_LABEL : S.EXECUTOR_LABEL
    const card = await $(roleLabel).$('..//..//..')

    const trigger = await card.$(S.MODEL_SEARCH)
    if (trigger) {
      await trigger.click()
    }

    const search = await $(S.MODEL_SEARCH)
    if (await search.isDisplayed()) {
      await search.setValue(modelName)
    }

    const modelItem = await $(`button*=${modelName}`)
    if (await modelItem.isDisplayed()) {
      await modelItem.click()
    }
  }
}
