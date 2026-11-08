import './setup-dom.ts'
import '../../src/renderer/src/i18n.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { cleanup, render } from '@testing-library/react'
import { AcceptanceMessageBody } from '../../src/renderer/src/components/AcceptanceMessageBody.tsx'

afterEach(cleanup)

const record = {
  iteration: 2,
  risk: 'medium',
  summary: 'Login form validates email.',
  checks: [
    { name: 'npm test', status: 'passed', durationMs: 8200 },
    { name: 'npm run lint', status: 'failed', durationMs: 3100 }
  ]
}

test('acceptance record reads as words, not raw enums', () => {
  const verdict = {
    verdict: 'fail',
    risk: 'medium',
    confidence: 0.8,
    issues: ['lint fails'],
    evidence: ['npm run lint: 2 errors'],
    reasoning: 'Lint must pass.',
    summary: 'Fix the lint errors.',
    nextStep: { action: 'continue', instructions: ['Fix lint in LoginForm.tsx'] }
  }
  const { container } = render(
    <AcceptanceMessageBody content={JSON.stringify({ ...record, verdict })} />
  )
  const text = container.textContent ?? ''
  for (const label of [
    'Medium risk',
    'iteration 2',
    '1/2 checks passed',
    'Checks',
    'Changes needed'
  ]) {
    assert.ok(text.includes(label), `shows "${label}"`)
  }
  assert.ok(!/risk·|iter·|\[fail\]/.test(text), 'no raw enum chips')
})

test('an incomplete record verdict is dropped, the checks still render', () => {
  const { container } = render(
    <AcceptanceMessageBody
      content={JSON.stringify({ ...record, verdict: { verdict: 'fail', summary: 'x' } })}
    />
  )
  const text = container.textContent ?? ''
  assert.ok(text.includes('1/2 checks passed'))
  assert.ok(!text.includes('Changes needed'))
})

test('a standalone verdict names the next step', () => {
  const verdict = {
    verdict: 'pass',
    risk: 'low',
    confidence: 0.9,
    issues: [],
    evidence: ['npm test passed'],
    reasoning: 'ok',
    summary: 'Ready to ship.',
    nextStep: { action: 'finish', instructions: [] }
  }
  const { container } = render(<AcceptanceMessageBody content={JSON.stringify(verdict)} />)
  const text = container.textContent ?? ''
  for (const label of ['Accepted', 'Low risk', 'Next: finish', 'Evidence']) {
    assert.ok(text.includes(label), `shows "${label}"`)
  }
})
