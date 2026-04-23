import type { AcceptanceRecord, AcceptanceVerdict } from '../types'

function extractJsonCandidates(raw: string): string[] {
  const trimmed = raw.trim()
  const candidates = new Set<string>()

  if (trimmed) {
    candidates.add(trimmed)
  }

  const fenceMatch = trimmed.match(/^```(?:json)?\s*([\s\S]*?)\s*```$/i)
  if (fenceMatch?.[1]) {
    candidates.add(fenceMatch[1].trim())
  }

  for (let i = 0; i < trimmed.length; i += 1) {
    if (trimmed[i] !== '{') continue
    let depth = 0
    let inString = false
    let escaped = false

    for (let j = i; j < trimmed.length; j += 1) {
      const char = trimmed[j]

      if (inString) {
        if (escaped) {
          escaped = false
          continue
        }
        if (char === '\\') {
          escaped = true
          continue
        }
        if (char === '"') {
          inString = false
        }
        continue
      }

      if (char === '"') {
        inString = true
        continue
      }
      if (char === '{') {
        depth += 1
      } else if (char === '}') {
        depth -= 1
        if (depth === 0) {
          candidates.add(trimmed.slice(i, j + 1).trim())
          break
        }
      }
    }
  }

  return [...candidates]
}

function isRisk(value: unknown): value is AcceptanceVerdict['risk'] {
  return value === 'low' || value === 'medium' || value === 'high'
}

function isDecision(value: unknown): value is AcceptanceVerdict['verdict'] {
  return value === 'pass' || value === 'fail'
}

function isAction(value: unknown): value is AcceptanceVerdict['nextStep']['action'] {
  return value === 'continue' || value === 'finish'
}

function normalizeVerdict(parsed: unknown): AcceptanceVerdict {
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('Acceptance verdict must be a JSON object')
  }

  const record = parsed as Record<string, unknown>
  const verdict = record.verdict
  const risk = record.risk
  const evidence = record.evidence
  const summary = record.summary
  const nextStep = record.nextStep

  if (!isDecision(verdict)) {
    throw new Error('Acceptance verdict is missing a valid `verdict` field')
  }
  if (!isRisk(risk)) {
    throw new Error('Acceptance verdict is missing a valid `risk` field')
  }
  if (!Array.isArray(evidence) || evidence.some((item) => typeof item !== 'string')) {
    throw new Error('Acceptance verdict must include string `evidence` items')
  }
  if (typeof summary !== 'string' || !summary.trim()) {
    throw new Error('Acceptance verdict must include a non-empty `summary`')
  }
  if (!nextStep || typeof nextStep !== 'object') {
    throw new Error('Acceptance verdict must include `nextStep`')
  }

  const action = (nextStep as Record<string, unknown>).action
  const instructions = (nextStep as Record<string, unknown>).instructions

  if (!isAction(action)) {
    throw new Error('Acceptance verdict must include a valid `nextStep.action`')
  }
  if (!Array.isArray(instructions) || instructions.some((item) => typeof item !== 'string')) {
    throw new Error('Acceptance verdict must include string `nextStep.instructions`')
  }
  if (action === 'continue' && instructions.length === 0) {
    throw new Error('Acceptance verdict requires follow-up instructions for continue')
  }
  if (action === 'finish' && instructions.length > 0) {
    throw new Error('Acceptance verdict cannot include instructions when action is finish')
  }

  return {
    verdict,
    risk,
    evidence,
    summary: summary.trim(),
    nextStep: {
      action,
      instructions: instructions.map((item) => item.trim()).filter(Boolean)
    }
  }
}

export function parseAcceptanceVerdict(raw: string): AcceptanceVerdict {
  let lastError = 'Acceptance verdict was empty'
  for (const candidate of extractJsonCandidates(raw)) {
    try {
      return normalizeVerdict(JSON.parse(candidate))
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error)
    }
  }
  throw new Error(lastError)
}

export function isAcceptanceVerdictContent(raw: string): boolean {
  try {
    parseAcceptanceVerdict(raw)
    return true
  } catch {
    return false
  }
}

export function buildMentorAcceptancePrompt(input: {
  taskSpec: string
  executorResult: string
  acceptance: AcceptanceRecord
}): string {
  const sections = [
    '### ROLE: MENTOR',
    'Your mission is ONLY to REVIEW and emit a structured acceptance verdict.',
    '- DO NOT execute commands or edit files.',
    '- Return STRICT JSON ONLY. No markdown, no prose, no code fences.',
    '- Use exactly this schema:',
    '{',
    '  "verdict": "pass | fail",',
    '  "risk": "low | medium | high",',
    '  "evidence": ["..."],',
    '  "summary": "...",',
    '  "nextStep": {',
    '    "action": "continue | finish",',
    '    "instructions": ["..."]',
    '  }',
    '}',
    '- If action is "continue", include concrete executor instructions.',
    '- If action is "finish", instructions must be an empty array.',
    '',
    '### TASK SPEC',
    input.taskSpec.trim(),
    '',
    '### EXECUTOR RESULT',
    input.executorResult.trim(),
    '',
    '### ACCEPTANCE REPORT',
    JSON.stringify(input.acceptance, null, 2)
  ]

  return sections.join('\n')
}

export function buildMentorAcceptanceRepairPrompt(error: string): string {
  return [
    '### ROLE: MENTOR',
    'Your last review output was not valid acceptance JSON.',
    '- Return STRICT JSON ONLY.',
    '- Do not include markdown, prose, or code fences.',
    `Validation error: ${error.trim()}`,
    '',
    'Return the corrected acceptance verdict now.'
  ].join('\n')
}

export function buildExecutorAcceptanceFollowupPrompt(input: {
  taskSpec: string
  previousExecutorResult: string
  verdict: AcceptanceVerdict
  acceptance: AcceptanceRecord
}): string {
  const instructions = input.verdict.nextStep.instructions
    .map((step, index) => `${index + 1}. ${step}`)
    .join('\n')

  return [
    '### ROLE: EXECUTOR',
    'Your mission is ONLY to EXECUTE the acceptance follow-up.',
    '- DO NOT create a new plan.',
    '- DO NOT review your own work.',
    '- Apply the mentor follow-up instructions exactly, then report what changed.',
    '- If a requested tool or method is unavailable to you, immediately continue with alternative text-based approaches instead of stopping. Report the limitation and proceed with what you CAN do.',
    '',
    '### TASK SPEC',
    input.taskSpec.trim(),
    '',
    '### PREVIOUS EXECUTOR RESULT',
    input.previousExecutorResult.trim(),
    '',
    '### MENTOR ACCEPTANCE VERDICT',
    JSON.stringify(input.verdict, null, 2),
    '',
    '### ACCEPTANCE REPORT',
    JSON.stringify(input.acceptance, null, 2),
    '',
    '### FOLLOW-UP INSTRUCTIONS',
    instructions
  ].join('\n')
}
