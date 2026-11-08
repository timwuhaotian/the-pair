/**
 * Preset picker: the info popover explains what a preset brings with it —
 * its skills, where it pauses the pair and whether it snapshots the repo.
 */
import './setup-dom.ts'
import './mock-ipc.ts'
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { I18nextProvider } from 'react-i18next'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import { PresetPicker } from '../../src/renderer/src/components/PresetPicker.tsx'
import type { PairPreset } from '../../src/renderer/src/types.ts'

void i18n.changeLanguage('en')

afterEach(() => cleanup())

const preset: PairPreset = {
  id: 'bug-fix',
  name: 'Bug Fix',
  description: 'Reproduce the bug, then fix it',
  icon: 'Bug',
  mentorPromptTemplate: 'TASK:\n{task}',
  executorPromptTemplate: 'execute',
  recommendedSkills: ['repro', 'regression'],
  pauseOnIteration: 5,
  autoAttachGitBaseline: true
}

function wrap(node: React.ReactNode): React.ReactNode {
  return <I18nextProvider i18n={i18n}>{node}</I18nextProvider>
}

test('the preset info popover lists the skills, the pause point and the baseline', async () => {
  const { container } = render(
    wrap(<PresetPicker presets={[preset]} selectedPresetId={null} onSelect={() => undefined} />)
  )

  // The popover opens on hover over the preset's info button.
  const trigger = container.querySelector('[data-preset-popover]')
  assert.ok(trigger)
  fireEvent.mouseEnter(trigger)

  await waitFor(() => assert.ok(screen.getByText('Reproduce the bug, then fix it')))
  assert.ok(screen.getByText('repro'))
  assert.ok(screen.getByText('regression'))
  assert.ok(screen.getByText(/Auto-pause at iteration 5/))
  assert.ok(screen.getByText(/Git baseline created when the pair starts/))
})

test('a preset can be selected and deselected', () => {
  let selected: PairPreset | null = null
  render(
    wrap(
      <PresetPicker
        presets={[preset]}
        selectedPresetId={selected?.id ?? null}
        onSelect={(next) => {
          selected = next
        }}
      />
    )
  )

  const card = screen.getByTestId('preset-card-bug-fix')
  fireEvent.click(card)
  assert.equal(selected, preset)
})
