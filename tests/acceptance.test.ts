import assert from 'node:assert/strict'
import test from 'node:test'

import {
  buildExecutorAcceptanceFollowupPrompt,
  buildMentorAcceptanceRepairPrompt,
  buildMentorAcceptancePrompt,
  isAcceptanceVerdictContent,
  parseAcceptanceVerdict
} from '../src/renderer/src/lib/acceptance.ts'
import type { AcceptanceRecord } from '../src/renderer/src/types.ts'

function sampleAcceptanceRecord(): AcceptanceRecord {
  return {
    iteration: 2,
    risk: 'medium',
    checks: [
      {
        name: 'git diff --check',
        command: 'git diff --check',
        status: 'passed',
        exitCode: 0,
        durationMs: 120,
        summary: 'diff check passed',
        stdout: '',
        stderr: ''
      },
      {
        name: 'npm run typecheck',
        command: 'npm run typecheck',
        status: 'failed',
        exitCode: 2,
        durationMs: 3100,
        summary: 'typecheck failed',
        stdout: '',
        stderr: 'src/App.tsx:10 error TS2322'
      }
    ],
    summary: '1 passed, 1 failed',
    startedAt: 100,
    finishedAt: 200
  }
}

test('parseAcceptanceVerdict accepts strict JSON with next-step instructions', () => {
  const verdict = parseAcceptanceVerdict(`{
    "verdict": "fail",
    "risk": "high",
    "evidence": ["npm run typecheck failed", "src/App.tsx still has TS2322"],
    "summary": "The iteration is not ready yet",
    "nextStep": {
      "action": "continue",
      "instructions": ["Fix the TS2322 error in App.tsx", "Re-run typecheck before handing back"]
    }
  }`)

  assert.equal(verdict.verdict, 'fail')
  assert.equal(verdict.risk, 'high')
  assert.deepEqual(verdict.issues, [])
  assert.deepEqual(verdict.evidence, ['npm run typecheck failed', 'src/App.tsx still has TS2322'])
  assert.equal(verdict.reasoning, 'The iteration is not ready yet')
  assert.equal(verdict.summary, 'The iteration is not ready yet')
  assert.deepEqual(verdict.nextStep, {
    action: 'continue',
    instructions: ['Fix the TS2322 error in App.tsx', 'Re-run typecheck before handing back']
  })
})

test('parseAcceptanceVerdict extracts JSON embedded in prose', () => {
  const verdict = parseAcceptanceVerdict(`Review complete.
  {
    "verdict": "pass",
    "risk": "low",
    "evidence": ["git diff --check passed"],
    "summary": "Everything needed for this task is in place",
    "nextStep": {
      "action": "finish",
      "instructions": []
    }
  }
  End of review.`)

  assert.equal(verdict.verdict, 'pass')
  assert.equal(verdict.nextStep.action, 'finish')
  assert.deepEqual(verdict.nextStep.instructions, [])
})

test('isAcceptanceVerdictContent detects valid verdict JSON embedded in text', () => {
  assert.equal(
    isAcceptanceVerdictContent(`Review complete.
    {
      "verdict": "pass",
      "risk": "low",
      "evidence": ["git diff --check passed"],
      "summary": "Everything needed for this task is in place",
      "nextStep": {
        "action": "finish",
        "instructions": []
      }
    }`),
    true
  )
})

test('isAcceptanceVerdictContent rejects ordinary mentor markdown', () => {
  assert.equal(
    isAcceptanceVerdictContent('## Plan\n\n1. Inspect the bug\n2. Patch the renderer'),
    false
  )
})

test('parseAcceptanceVerdict rejects pass verdicts that request continuation', () => {
  assert.throws(
    () =>
      parseAcceptanceVerdict(`{
        "verdict": "pass",
        "risk": "low",
        "confidence": 0.95,
        "issues": [],
        "evidence": ["Only one chat round completed"],
        "summary": "More rounds are required",
        "nextStep": {
          "action": "continue",
          "instructions": ["Send round 2"]
        }
      }`),
    /pass.*finish/i
  )
})

test('parseAcceptanceVerdict rejects fail verdicts that request finish', () => {
  assert.throws(
    () =>
      parseAcceptanceVerdict(`{
        "verdict": "fail",
        "risk": "low",
        "confidence": 0.65,
        "issues": ["Task incomplete"],
        "evidence": ["Only one chat round completed"],
        "summary": "More rounds are required",
        "nextStep": {
          "action": "finish",
          "instructions": []
        }
      }`),
    /fail.*continue/i
  )
})

test('buildMentorAcceptancePrompt embeds executor result and acceptance report', () => {
  const prompt = buildMentorAcceptancePrompt({
    taskSpec: 'Implement structured acceptance gates',
    executorResult: 'Implemented the backend types and parser',
    acceptance: sampleAcceptanceRecord()
  })

  assert.match(prompt, /assessment as a JSON block/i)
  assert.match(prompt, /Implement structured acceptance gates/)
  assert.match(prompt, /Implemented the backend types and parser/)
  assert.match(prompt, /"command": "npm run typecheck"/)
  assert.match(prompt, /"action": "continue" \| "finish"/)
  assert.match(prompt, /TASK_COMPLETE/)
  // Should not contain role-play or anti-introspection markers that trip model safety filters.
  assert.doesNotMatch(prompt, /### ROLE:/)
  assert.doesNotMatch(prompt, /DO NOT/)
})

test('buildExecutorAcceptanceFollowupPrompt uses structured next-step instructions', () => {
  const prompt = buildExecutorAcceptanceFollowupPrompt({
    taskSpec: 'Implement structured acceptance gates',
    previousExecutorResult: 'Added the first draft',
    verdict: {
      verdict: 'fail',
      risk: 'medium',
      evidence: ['typecheck failed'],
      summary: 'Needs one more pass',
      nextStep: {
        action: 'continue',
        instructions: ['Fix the type error in App.tsx', 'Run npm run typecheck']
      }
    },
    acceptance: sampleAcceptanceRecord()
  })

  assert.match(prompt, /adjustments to your previous turn/i)
  assert.match(prompt, /Fix the type error in App.tsx/)
  assert.match(prompt, /Run npm run typecheck/)
  assert.doesNotMatch(prompt, /typecheck failed/)
  assert.doesNotMatch(prompt, /Needs one more pass/)
  assert.doesNotMatch(prompt, /Added the first draft/)
  assert.match(prompt, /Do not append TASK_COMPLETE/i)
})

test('buildExecutorAcceptanceFollowupPrompt requires exact text-only follow-up output', () => {
  const prompt = buildExecutorAcceptanceFollowupPrompt({
    taskSpec: 'Smoke greeting task',
    previousExecutorResult: 'Greeting 2/3',
    verdict: {
      verdict: 'fail',
      risk: 'low',
      evidence: ['Greeting 2/3 received'],
      summary: 'One more greeting is required',
      nextStep: {
        action: 'continue',
        instructions: ['Send Greeting 3/3']
      }
    },
    acceptance: sampleAcceptanceRecord()
  })

  assert.match(prompt, /output exactly the text the reviewer asked for/i)
  assert.match(prompt, /Do not append TASK_COMPLETE/i)
  assert.doesNotMatch(prompt, /received/i)
  assert.match(prompt, /TASK_COMPLETE/)
  assert.doesNotMatch(prompt, /### ROLE:/)
})

test('buildMentorAcceptanceRepairPrompt explains the JSON validation error', () => {
  const prompt = buildMentorAcceptanceRepairPrompt(
    'Acceptance verdict requires follow-up instructions for continue'
  )

  assert.match(prompt, /couldn't parse your previous reply/i)
  assert.match(prompt, /JSON/i)
  assert.match(prompt, /requires follow-up instructions for continue/)
  assert.doesNotMatch(prompt, /### ROLE:/)
})
