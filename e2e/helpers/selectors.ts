export const S = {
  TOOLBAR: '.app-chrome',
  NEW_PAIR_BTN: '[data-testid="sidebar-new-pair"]',
  NEW_TASK_BTN: '[data-testid="chrome-new-task"]',
  CLEAR_SESSION_BTN: '[data-testid="chrome-clear-session"]',
  MODELS_BTN: '[data-testid="chrome-models"]',
  BACK_BTN: '[data-testid="chrome-back"]',
  THEME_TOGGLE: '[data-testid="chrome-theme-toggle"]',

  STATUS: (status: string) => `span*=${status}`,

  PAIR_CARD: (name: string) => `div*=${name}`,

  MODAL_TITLE: 'h2',
  MODAL_CLOSE_BTN: '.glass-modal button',
  MODAL_BACKDROP: 'div[class*="bg-black"]',

  NAME_INPUT: '[data-testid="pair-name-input"]',
  DIRECTORY_INPUT: '[data-testid="pair-directory-input"]',
  TASK_SPEC_CREATE: '[data-testid="pair-task-spec"]',
  CREATE_PAIR_SUBMIT: '[data-testid="pair-submit-btn"]',
  CANCEL_BTN_CREATE: '[data-testid="pair-cancel-btn"]',

  TASK_SPEC_ASSIGN: '[data-testid="assign-task-spec"]',
  START_NEW_TASK_BTN: '[data-testid="assign-submit-btn"]',
  CANCEL_BTN_ASSIGN: '[data-testid="assign-cancel-btn"]',

  SAVE_DEFAULTS_BTN: '[data-testid="settings-save-btn"]',

  MENTOR_LABEL: 'div*=Mentor',
  EXECUTOR_LABEL: 'div*=Executor',
  MODEL_SEARCH: 'input[placeholder*="Search models"]',

  PAUSE_BTN: '=Pause Pair',
  RESUME_BTN: '=Resume Pair',
  RETRY_BTN: '=Retry Turn',

  CONSOLE_PANEL: '.overflow-y-auto',

  ERROR_PANEL: '[class*="text-destructive"]',

  CANCEL_BTN: '=Cancel'
} as const

export function modalTitle(title: string): string {
  return `h2*=${title}`
}
