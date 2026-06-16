export interface ExecutorHandoffPromptInput {
  mentorPlan?: string | null
}

export interface MentorReviewPromptInput {
  executorOutput?: string | null
}

const EXECUTOR_PROMPT_HEADER =
  "You're collaborating with another AI agent in an automated pair-programming workflow. " +
  'Carry out the plan below — treat it as direct actions to perform right now, not a roadmap to comment on.\n\n' +
  'A few constraints:\n' +
  '- Do the next concrete action the plan calls for; do not restate, summarize, or narrate the plan back.\n' +
  '- If the plan asks for specific output text, reply with exactly that text — no preface, no commentary, no status reports like "awaiting…" or "instruction is set to…".\n' +
  '- Just execute the steps; the reviewer will check the work after.\n' +
  "- The workflow decides when to stop, not you — don't add TASK_COMPLETE to your reply.\n" +
  "- If a tool isn't available, fall back to the closest text-based equivalent rather than stopping.\n\n" +
  'PLAN\n'

const MISSING_PLAN_FALLBACK =
  'No plan has been provided yet. Briefly summarise the current state and ask the reviewer for a plan.'

export function buildInitialExecutorHandoffPrompt({
  mentorPlan
}: ExecutorHandoffPromptInput): string {
  const planBody = mentorPlan?.trim()
  return (
    EXECUTOR_PROMPT_HEADER + (planBody && planBody.length > 0 ? planBody : MISSING_PLAN_FALLBACK)
  )
}

const MENTOR_PROMPT_HEADER =
  "You're collaborating with another AI agent in an automated pair-programming workflow. " +
  'The executor just finished a turn. Read its output below and decide whether the task is done or needs another pass.\n\n' +
  'For this review turn, focus on analysis — no need to run commands or edit files.\n\n' +
  'EXECUTOR OUTPUT\n'

const MENTOR_PROMPT_FOOTER =
  "If you're satisfied the task is complete, include TASK_COMPLETE somewhere in your reply so the orchestrator stops the workflow. Otherwise, write a refined plan describing what the executor should do next."

export function buildInitialMentorReviewPrompt({
  executorOutput
}: MentorReviewPromptInput): string {
  const outputBody = executorOutput?.trim()
  const body = outputBody && outputBody.length > 0 ? `${outputBody}\n\n` : ''
  return MENTOR_PROMPT_HEADER + body + MENTOR_PROMPT_FOOTER
}

export interface PlanRevisionPromptInput {
  taskSpec: string
  previousPlan?: string | null
  feedback?: string | null
}

const PLAN_REVISION_HEADER =
  "You're collaborating with another AI agent in a pair-programming workflow. " +
  'A human reviewed your previous plan and sent it back for revision before the ' +
  'executor starts. Produce a revised plan that addresses their feedback.\n\n' +
  'For this turn, focus on planning — no need to run commands or edit files. ' +
  "Don't add TASK_COMPLETE; the workflow decides when to stop.\n\n"

/**
 * Builds the mentor re-plan prompt used when a human rejects a gated plan.
 * Threads the original task, the previous plan, and the human's feedback so the
 * mentor revises rather than starts from scratch.
 */
export function buildPlanRevisionPrompt({
  taskSpec,
  previousPlan,
  feedback
}: PlanRevisionPromptInput): string {
  const sections = [`${PLAN_REVISION_HEADER}TASK\n${taskSpec.trim()}`]
  const plan = previousPlan?.trim()
  if (plan && plan.length > 0) {
    sections.push(`PREVIOUS PLAN\n${plan}`)
  }
  const note = feedback?.trim()
  sections.push(
    `HUMAN FEEDBACK\n${note && note.length > 0 ? note : '(no specific notes — improve the plan)'}`
  )
  return sections.join('\n\n')
}
