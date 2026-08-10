export const S = {
  // ── Toolbar / Chrome ──────────────────────────────────────────────
  TOOLBAR: '.app-chrome',
  NEW_PAIR_BTN: '[data-testid="sidebar-new-pair"]',
  NEW_TASK_BTN: '[data-testid="chrome-new-task"]',
  CLEAR_SESSION_BTN: '[data-testid="chrome-clear-session"]',
  MODELS_BTN: '[data-testid="chrome-models"]',
  BACK_BTN: '[data-testid="chrome-back"]',
  THEME_TOGGLE: '[data-testid="chrome-theme-toggle"]',
  SHORTCUTS_BTN: '[data-testid="chrome-shortcuts"]',
  MUTE_TOGGLE: '[data-testid="chrome-mute-toggle"]',
  LANGUAGE_TOGGLE: '[data-testid="chrome-language-toggle"]',

  // ── Status / Pair Cards ───────────────────────────────────────────
  STATUS: (status: string) => `span*=${status}`,
  PAIR_CARD: (name: string) => `div*=${name}`,
  PAIR_CARD_DELETE: (id: string) => `[data-testid="pair-card-delete-${id}"]`,

  // ── Modal Shell ───────────────────────────────────────────────────
  MODAL_TITLE: 'h2',
  MODAL_CLOSE_BTN: '.glass-modal button',
  MODAL_BACKDROP: 'div[class*="bg-black"]',

  // ── Create Pair Modal ─────────────────────────────────────────────
  NAME_INPUT: '[data-testid="pair-name-input"]',
  DIRECTORY_INPUT: '[data-testid="pair-directory-input"]',
  TASK_SPEC_CREATE: '[data-testid="pair-task-spec"]',
  CREATE_PAIR_SUBMIT: '[data-testid="pair-submit-btn"]',
  CANCEL_BTN_CREATE: '[data-testid="pair-cancel-btn"]',
  PLAN_GATE_TOGGLE: '[data-testid="plan-gate-toggle"]',

  // ── Assign Task Modal ─────────────────────────────────────────────
  TASK_SPEC_ASSIGN: '[data-testid="assign-task-spec"]',
  START_NEW_TASK_BTN: '[data-testid="assign-submit-btn"]',
  CANCEL_BTN_ASSIGN: '[data-testid="assign-cancel-btn"]',

  // ── Settings Modal ────────────────────────────────────────────────
  SAVE_DEFAULTS_BTN: '[data-testid="settings-save-btn"]',
  CANCEL_BTN_SETTINGS: '[data-testid="settings-cancel-btn"]',

  // ── Model Picker ──────────────────────────────────────────────────
  MENTOR_LABEL: 'div*=Mentor',
  EXECUTOR_LABEL: 'div*=Executor',
  MODEL_SEARCH: 'input[placeholder*="Search models"]',

  // ── Operations Panel ──────────────────────────────────────────────
  PAUSE_BTN: '[data-testid="ops-pause-btn"]',
  RESUME_BTN: '[data-testid="ops-resume-btn"]',
  RETRY_BTN: '[data-testid="ops-retry-btn"]',
  ITERATION_PROGRESS: '[data-testid="iteration-progress"]',

  // ── Console ───────────────────────────────────────────────────────
  CONSOLE_PANEL: '.overflow-y-auto',
  CONSOLE_STOP_TURN_BTN: '[data-testid="console-stop-turn-btn"]',
  CONSOLE_CLEAR_BTN: '[data-testid="console-clear-btn"]',
  CONSOLE_TASK_INPUT: '[data-testid="pair-task-input"]',
  CONSOLE_TASK_TEXTAREA: '[data-testid="pair-task-input"] textarea',
  FILTER_ALL: '[data-testid="filter-all"]',
  FILTER_MENTOR: '[data-testid="filter-mentor"]',
  FILTER_EXECUTOR: '[data-testid="filter-executor"]',

  // ── Plan Review ───────────────────────────────────────────────────
  PLAN_APPROVE_BTN: '[data-testid="plan-approve-btn"]',
  PLAN_REJECT_BTN: '[data-testid="plan-reject-btn"]',
  PLAN_SENDBACK_BTN: '[data-testid="plan-sendback-btn"]',
  PLAN_FEEDBACK_INPUT: '[data-testid="plan-feedback-input"]',

  // ── Confirm Modal ─────────────────────────────────────────────────
  CONFIRM_MODAL_CONFIRM: '[data-testid="confirm-modal-confirm"]',
  CONFIRM_MODAL_CANCEL: '[data-testid="confirm-modal-cancel"]',

  // ── Error Panel ───────────────────────────────────────────────────
  ERROR_PANEL: '[class*="text-destructive"]',
  ERROR_RETRY_BTN: '[data-testid="error-retry-btn"]',

  // ── Generic ───────────────────────────────────────────────────────
  CANCEL_BTN: '=Cancel'
} as const

export function modalTitle(title: string): string {
  return `h2*=${title}`
}
