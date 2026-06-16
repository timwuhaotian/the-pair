import assert from 'node:assert/strict'
import test from 'node:test'

import {
  buildInitialExecutorHandoffPrompt,
  buildInitialMentorReviewPrompt,
  buildPlanRevisionPrompt
} from '../src/renderer/src/lib/handoffPrompts.ts'

test('executor handoff prompt frames the plan as direct actions, not a roadmap to narrate', () => {
  const prompt = buildInitialExecutorHandoffPrompt({
    mentorPlan: '1. Set executor instruction to "Send Greeting 1/3".\n2. Wait for executor.'
  })

  assert.match(prompt, /direct actions/i)
  assert.match(prompt, /do not restate, summarize, or narrate the plan/i)
  assert.match(prompt, /reply with exactly that text/i)
  assert.match(prompt, /no status reports like/i)
  assert.match(prompt, /TASK_COMPLETE/)
  assert.match(prompt, /PLAN\n/)
  assert.ok(prompt.includes('Send Greeting 1/3'), 'mentor plan body should be embedded verbatim')
})

test('executor handoff prompt falls back to a recovery instruction when no plan is available', () => {
  const prompt = buildInitialExecutorHandoffPrompt({ mentorPlan: null })

  assert.match(prompt, /No plan has been provided yet/)
  assert.match(prompt, /ask the reviewer for a plan/)
})

test('executor handoff prompt treats whitespace-only plans as missing', () => {
  const prompt = buildInitialExecutorHandoffPrompt({ mentorPlan: '   \n\n  ' })

  assert.match(prompt, /No plan has been provided yet/)
})

test('mentor review prompt embeds the executor output and asks for TASK_COMPLETE on success', () => {
  const prompt = buildInitialMentorReviewPrompt({ executorOutput: 'Greeting 1/3' })

  assert.match(prompt, /EXECUTOR OUTPUT\nGreeting 1\/3/)
  assert.match(prompt, /TASK_COMPLETE/)
  assert.match(prompt, /refined plan/)
})

test('mentor review prompt is well-formed even when executor output is missing', () => {
  const prompt = buildInitialMentorReviewPrompt({ executorOutput: null })

  assert.match(prompt, /EXECUTOR OUTPUT\n/)
  assert.match(prompt, /TASK_COMPLETE/)
  assert.doesNotMatch(prompt, /undefined/i)
  assert.doesNotMatch(prompt, /null/i)
})

test('plan revision prompt threads task, previous plan, and human feedback', () => {
  const prompt = buildPlanRevisionPrompt({
    taskSpec: 'Add a logout button',
    previousPlan: '1. Edit Navbar.tsx',
    feedback: 'Put it in the user menu, not the navbar'
  })

  assert.match(prompt, /TASK\nAdd a logout button/)
  assert.match(prompt, /PREVIOUS PLAN\n1\. Edit Navbar\.tsx/)
  assert.match(prompt, /HUMAN FEEDBACK\nPut it in the user menu/)
  assert.match(prompt, /revis/i)
})

test('plan revision prompt is well-formed when feedback and previous plan are omitted', () => {
  const prompt = buildPlanRevisionPrompt({ taskSpec: 'Add a logout button' })

  assert.match(prompt, /TASK\nAdd a logout button/)
  assert.doesNotMatch(prompt, /PREVIOUS PLAN/)
  assert.match(prompt, /HUMAN FEEDBACK\n\(no specific notes/)
  assert.doesNotMatch(prompt, /undefined/i)
  assert.doesNotMatch(prompt, /null/i)
})
