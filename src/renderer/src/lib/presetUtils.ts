import type { PairPreset } from '../types'

const DEV_SMOKE_TEST_PRESET: PairPreset = {
  id: 'dev-smoke-test',
  name: 'Dev Smoke Test',
  description:
    'Local dev only. Executor sends Greeting 1/3, 2/3, 3/3 (one per turn), then mentor marks done. Verifies the full pair lifecycle in ~4 iterations.',
  icon: 'FlaskConical',
  mentorPromptTemplate:
    'This is a smoke test of the pair execution loop, used to verify the workflow end-to-end. Each time the executor sends a greeting, reply with "Greeting N/3 received." (where N is 1, 2, or 3). After greeting 3 is acknowledged, the workflow is done.\n\nFirst mentor planning turn: set the executor instruction to "Send Greeting 1/3". Do not acknowledge any greeting until the executor has actually sent it.\n\nReview turn rules:\n- If the executor sent "Greeting 1/3", acknowledge "Greeting 1/3 received", return a fail/continue review, and set the next instruction to "Send Greeting 2/3".\n- If the executor sent "Greeting 2/3", acknowledge "Greeting 2/3 received", return a fail/continue review, and set the next instruction to "Send Greeting 3/3".\n- If the executor sent "Greeting 3/3", acknowledge "Greeting 3/3 received", return a pass/finish review, and put TASK_COMPLETE on its own line.\n\nNo code execution, file edits, or tools are needed for this test — just drive the greeting handoff protocol.',
  executorPromptTemplate:
    'This is a smoke test of the pair execution loop, used to verify the workflow end-to-end. Each turn, send exactly one short greeting:\n- Turn 1: "Greeting 1/3"\n- Turn 2: "Greeting 2/3"\n- Turn 3: "Greeting 3/3"\n\nReply with just the greeting text — no tools, no commands, no extra commentary.',
  recommendedSkills: [],
  pauseOnIteration: 4
}

export const HARDCODED_PRESETS: PairPreset[] = [
  {
    id: 'bug-fix',
    name: 'Bug Fix',
    description:
      'Quickly investigate and fix a specific bug. Auto-pauses at iteration 5 for review.',
    icon: 'Bug',
    mentorPromptTemplate:
      'You are a meticulous bug investigator. Your role is to:\n1. Analyze the reported issue\n2. Identify root cause\n3. Propose a fix\n\nTASK:\n{task}\n\nBe systematic. Check edge cases. Verify your fix before presenting it.',
    executorPromptTemplate:
      'You are a precise bug fixer. Execute the fix as specified by the mentor.',
    recommendedSkills: [],
    pauseOnIteration: 5
  },
  {
    id: 'refactor',
    name: 'Refactor',
    description: 'Safely improve code structure. Creates git baseline for rollback.',
    icon: 'RefreshCw',
    mentorPromptTemplate:
      'You are a refactoring mentor. Guide safe, incremental improvements:\n1. Understand current structure\n2. Identify improvement opportunities\n3. Propose small, safe changes\n\nTASK:\n{task}\n\nPrioritize clarity and maintainability. Never break existing behavior.',
    executorPromptTemplate: 'Execute refactoring changes as guided. Run tests after each change.',
    recommendedSkills: [],
    pauseOnIteration: 8,
    autoAttachGitBaseline: true
  },
  {
    id: 'feature',
    name: 'Feature',
    description: 'Build new functionality end-to-end with planning and review.',
    icon: 'Sparkles',
    mentorPromptTemplate:
      'You are a feature planning mentor. Help break down and build:\n1. Understand requirements thoroughly\n2. Plan the implementation approach\n3. Review each step\n\nTASK:\n{task}\n\nThink big picture but execute incrementally.',
    executorPromptTemplate:
      'Implement features as planned. Ask for clarification if requirements are unclear.',
    recommendedSkills: [],
    pauseOnIteration: 15
  },
  {
    id: 'hardening',
    name: 'Hardening',
    description: 'Improve error handling, security, and robustness.',
    icon: 'Shield',
    mentorPromptTemplate:
      'You are a hardening specialist. Improve code quality:\n1. Identify potential failure points\n2. Suggest defensive improvements\n3. Verify error handling\n\nTASK:\n{task}\n\nBe thorough. No bug is too small to fix.',
    executorPromptTemplate: 'Implement hardening improvements. Add tests for edge cases.',
    recommendedSkills: [],
    pauseOnIteration: 8
  }
]

export function buildSpecFromPreset(preset: PairPreset, userTask: string): string {
  const template = preset.mentorPromptTemplate
  if (!template.includes('{task}')) {
    return template
  }
  const taskText = userTask?.trim() || 'Describe what you want the pair to accomplish...'
  return template.replace('{task}', taskText)
}

export function stripTemplate(spec: string): string {
  const match = spec.match(/TASK:\s*([\s\S]*)$/)
  return match ? match[1].trim() : spec
}

export function getPresets(isDev: boolean): PairPreset[] {
  return isDev ? [...HARDCODED_PRESETS, DEV_SMOKE_TEST_PRESET] : HARDCODED_PRESETS
}
