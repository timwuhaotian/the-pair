import type {
  AcceptanceCheckRun,
  AcceptanceCheckStatus,
  AcceptanceRecord,
  AcceptanceRisk,
  AcceptanceVerdict
} from '../types'

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

function validateVerdictAction(
  verdict: AcceptanceVerdict['verdict'],
  action: AcceptanceVerdict['nextStep']['action']
): void {
  if (verdict === 'pass' && action !== 'finish') {
    throw new Error('Acceptance pass verdict must use nextStep.action finish')
  }
  if (verdict === 'fail' && action !== 'continue') {
    throw new Error('Acceptance fail verdict must use nextStep.action continue')
  }
}

function normalizeVerdict(parsed: unknown): AcceptanceVerdict {
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('Acceptance verdict must be a JSON object')
  }

  const record = parsed as Record<string, unknown>
  const verdict = record.verdict
  const risk = record.risk
  const confidence = record.confidence
  const issues = record.issues
  const evidence = record.evidence
  const reasoning = record.reasoning
  const summary = record.summary
  const nextStep = record.nextStep

  if (!isDecision(verdict)) {
    throw new Error('Acceptance verdict is missing a valid `verdict` field')
  }
  if (!isRisk(risk)) {
    throw new Error('Acceptance verdict is missing a valid `risk` field')
  }
  const confidenceValue =
    typeof confidence === 'number'
      ? confidence
      : typeof confidence === 'string'
        ? parseFloat(confidence)
        : verdict === 'pass'
          ? 1
          : 0

  if (!Number.isFinite(confidenceValue) || confidenceValue < 0 || confidenceValue > 1) {
    throw new Error('Acceptance verdict must include a valid `confidence` (0-1)')
  }

  const normalizedIssues = issues === undefined ? [] : issues
  if (
    !Array.isArray(normalizedIssues) ||
    normalizedIssues.some((item) => typeof item !== 'string')
  ) {
    throw new Error('Acceptance verdict must include string `issues` items')
  }
  if (!Array.isArray(evidence) || evidence.some((item) => typeof item !== 'string')) {
    throw new Error('Acceptance verdict must include string `evidence` items')
  }
  if (typeof summary !== 'string' || !summary.trim()) {
    throw new Error('Acceptance verdict must include a non-empty `summary`')
  }
  const normalizedReasoning =
    typeof reasoning === 'string' && reasoning.trim() ? reasoning.trim() : summary.trim()
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
  validateVerdictAction(verdict, action)

  return {
    verdict,
    risk,
    confidence: confidenceValue,
    issues: normalizedIssues.map((item) => item.trim()).filter(Boolean),
    evidence,
    reasoning: normalizedReasoning,
    summary: summary.trim(),
    nextStep: {
      action,
      instructions: instructions.map((item) => item.trim()).filter(Boolean)
    }
  }
}

function asString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}

function asStringArray(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined
  return value.every((entry) => typeof entry === 'string')
    ? value.map((entry) => entry.trim())
    : undefined
}

function extractNextStep(record: Record<string, unknown>): unknown {
  return record.nextStep ?? record.next_step ?? record['next-step']
}

function normalizeVerdictFallback(parsed: unknown): AcceptanceVerdict {
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('Acceptance verdict must be a JSON object')
  }

  const record = parsed as Record<string, unknown>
  const verdict = asString(record.verdict)
  const risk = asString(record.risk)
  const evidence = asStringArray(record.evidence)
  const issues = asStringArray(record.issues) ?? []
  const reasoning = asString(record.reasoning)
  const summary = asString(record.summary)
  const confidenceValue =
    typeof record.confidence === 'number'
      ? record.confidence
      : typeof record.confidence === 'string'
        ? parseFloat(record.confidence)
        : verdict === 'pass'
          ? 1
          : 0

  if (!isDecision(verdict)) {
    throw new Error('Acceptance verdict is missing a valid `verdict` field')
  }
  if (!isRisk(risk)) {
    throw new Error('Acceptance verdict is missing a valid `risk` field')
  }
  if (typeof confidenceValue !== 'number' || confidenceValue < 0 || confidenceValue > 1) {
    throw new Error('Acceptance verdict must include a valid `confidence` (0-1)')
  }
  if (!Array.isArray(evidence)) {
    throw new Error('Acceptance verdict must include string `evidence` items')
  }
  if (!summary) {
    throw new Error('Acceptance verdict must include a non-empty `summary`')
  }
  const normalizedReasoning = reasoning ?? summary

  const nextStep = extractNextStep(record)
  if (!nextStep || typeof nextStep !== 'object' || Array.isArray(nextStep)) {
    throw new Error('Acceptance verdict must include `nextStep`')
  }

  const nextStepRecord = nextStep as Record<string, unknown>
  const action = asString(nextStepRecord.action)
  const instructions = asStringArray(nextStepRecord.instructions)

  if (!isAction(action)) {
    throw new Error('Acceptance verdict must include a valid `nextStep.action`')
  }
  if (!Array.isArray(instructions) || instructions.some((item) => !item)) {
    throw new Error('Acceptance verdict must include string `nextStep.instructions`')
  }
  validateVerdictAction(verdict, action)

  return {
    verdict,
    risk,
    confidence: confidenceValue,
    issues,
    evidence,
    reasoning: normalizedReasoning,
    summary,
    nextStep: {
      action,
      instructions: instructions
    }
  }
}

export function parseAcceptanceVerdict(raw: string): AcceptanceVerdict {
  let lastError = 'Acceptance verdict was empty'
  for (const candidate of extractJsonCandidates(raw)) {
    let parsed: unknown
    try {
      parsed = JSON.parse(candidate)
      return normalizeVerdict(parsed)
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error)
      if (parsed && typeof parsed === 'object' && 'verdict' in parsed) {
        throw new Error(lastError)
      }
    }
  }
  throw new Error(lastError)
}

export function parseAcceptanceVerdictForDisplay(raw: string): AcceptanceVerdict {
  let lastError = 'Acceptance verdict was empty'
  for (const candidate of extractJsonCandidates(raw)) {
    try {
      return parseAcceptanceVerdict(candidate)
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error)
    }

    try {
      const parsed = JSON.parse(candidate)
      return normalizeVerdictFallback(parsed)
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error)
    }
  }

  throw new Error(lastError)
}

export function isAcceptanceVerdictContent(raw: string): boolean {
  try {
    parseAcceptanceVerdictForDisplay(raw)
    return true
  } catch {
    return false
  }
}

function isDevSmokePairSpec(spec: string): boolean {
  return (
    spec.includes('This is a smoke test of the pair execution loop') &&
    spec.includes('Each time the executor sends a greeting') &&
    spec.includes('Greeting N/3 received.')
  )
}

export function buildMentorAcceptancePrompt(input: {
  taskSpec: string
  executorResult: string
  acceptance: AcceptanceRecord
}): string {
  const isSmoke = isDevSmokePairSpec(input.taskSpec)

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
    ...(isSmoke
      ? [
          'SMOKE TEST MODE:',
          '- This is a 3-round greeting test. The task requires exactly 3 greetings.',
          '- Check what greeting number the Executor sent. Look for "Greeting 1", "Greeting 2", "Greeting 3" etc.',
          '- If greeting 1 or 2: FAIL verdict, risk=low, action=continue, instructions=["Send Greeting {N+1}/3"]',
          '- If greeting 3: PASS verdict, risk=low, action=finish, confidence=1.0, instructions=[]. After the JSON, output TASK_COMPLETE on its own line.',
          '- Include "Greeting N/3 received" in your response text (outside the JSON).',
          ''
        ]
      : []),
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
    '- Output exactly the requested instruction result.',
    '- For text-only instructions, return only that exact text.',
    '- Do not append acknowledgements, TASK_COMPLETE, explanations, or completion reports.',
    '- If a requested tool or method is unavailable to you, immediately continue with alternative text-based approaches instead of stopping. Briefly state the limitation only when it blocks exact execution.',
    '',
    '### FOLLOW-UP INSTRUCTIONS',
    instructions
  ].join('\n')
}

export interface ParsedAcceptanceRecord {
  iteration: number
  risk: AcceptanceRisk
  summary: string
  checks: Array<{
    name: string
    status: AcceptanceCheckStatus
    summary: string
    durationMs: number
  }>
  verdict?: AcceptanceVerdict
}

function normalizeAcceptanceCheck(raw: unknown): AcceptanceCheckRun | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  const name = typeof r.name === 'string' ? r.name : ''
  const command = typeof r.command === 'string' ? r.command : ''
  const status = r.status
  if (status !== 'passed' && status !== 'failed' && status !== 'skipped') return null
  return {
    name,
    command,
    status,
    exitCode: typeof r.exitCode === 'number' ? r.exitCode : null,
    durationMs: typeof r.durationMs === 'number' ? r.durationMs : 0,
    summary: typeof r.summary === 'string' ? r.summary : '',
    stdout: typeof r.stdout === 'string' ? r.stdout : '',
    stderr: typeof r.stderr === 'string' ? r.stderr : ''
  }
}

export function parseAcceptanceRecordForDisplay(raw: string): ParsedAcceptanceRecord {
  for (const candidate of extractJsonCandidates(raw)) {
    try {
      const parsed = JSON.parse(candidate)
      if (!parsed || typeof parsed !== 'object') continue
      const r = parsed as Record<string, unknown>

      if (!Array.isArray(r.checks)) continue
      const checks = r.checks
        .map(normalizeAcceptanceCheck)
        .filter((c): c is AcceptanceCheckRun => c !== null)
      if (checks.length === 0) continue

      if (typeof r.summary !== 'string' || !r.summary.trim()) continue
      if (typeof r.iteration !== 'number') continue
      if (!isRisk(r.risk)) continue

      let verdict: AcceptanceVerdict | undefined
      if (r.verdict && typeof r.verdict === 'object') {
        try {
          verdict = normalizeVerdict(r.verdict)
        } catch {
          verdict = undefined
        }
      }

      return {
        iteration: r.iteration,
        risk: r.risk,
        summary: r.summary.trim(),
        checks: checks.map((c) => ({
          name: c.name,
          status: c.status,
          summary: c.summary,
          durationMs: c.durationMs
        })),
        verdict
      }
    } catch {
      continue
    }
  }

  throw new Error('Could not parse AcceptanceRecord JSON')
}

export function isAcceptanceRecordContent(raw: string): boolean {
  try {
    parseAcceptanceRecordForDisplay(raw)
    return true
  } catch {
    return false
  }
}
