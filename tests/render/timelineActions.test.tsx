import './setup-dom.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { mockIPC } from '@tauri-apps/api/mocks'
import '../../src/renderer/src/tauri-shim.ts'
import '../../src/renderer/src/i18n.ts'
import { TimelineActions } from '../../src/renderer/src/components/TimelinePanel.tsx'
import { MessageCard } from '../../src/renderer/src/components/MessageCard.tsx'
import type { TimelineData } from '../../src/renderer/src/lib/timeline.ts'

afterEach(cleanup)

const timeline: TimelineData = {
  pairName: 'Pair',
  spec: 'Task',
  mentorModel: 'mentor',
  executorModel: 'executor',
  startedAt: 1,
  status: 'Finished',
  iterations: [],
  totalOutputTokens: 0,
  totalInputTokens: 0,
  mentorOutputTokens: 0,
  mentorInputTokens: 0,
  executorOutputTokens: 0,
  executorInputTokens: 0,
  acceptanceRecords: [],
  modifiedFiles: [],
  durationMs: 1
}

test('timeline copy and export failures reach the inspector error callback', async () => {
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText: async () => Promise.reject(new Error('clipboard denied')) }
  })
  mockIPC((cmd) => {
    if (cmd === 'plugin:dialog|save') throw new Error('dialog denied')
    return undefined
  })
  const errors: string[] = []
  render(<TimelineActions timeline={timeline} onError={(message) => errors.push(message)} />)
  const [copy, exportButton] = screen.getAllByRole('button')

  fireEvent.click(copy)
  await waitFor(() => assert.deepEqual(errors, ["Couldn't copy the report to the clipboard"]))
  fireEvent.click(exportButton)
  await waitFor(() =>
    assert.deepEqual(errors, [
      "Couldn't copy the report to the clipboard",
      "Couldn't export the report"
    ])
  )
})

test('handoff messages render a visible divider after all hooks', () => {
  const { container } = render(
    <MessageCard
      msg={{
        id: 'handoff-1',
        timestamp: 1,
        from: 'mentor',
        to: 'executor',
        type: 'handoff',
        content: 'next',
        iteration: 1
      }}
    />
  )
  assert.ok(container.querySelector('.tty-divider-dashed'))
})
