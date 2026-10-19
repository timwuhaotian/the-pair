import './setup-dom.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { GlassButton } from '../../src/renderer/src/components/ui/GlassButton.tsx'
import { IconButton } from '../../src/renderer/src/components/ui/IconButton.tsx'
import { Chip } from '../../src/renderer/src/components/ui/Chip.tsx'

afterEach(cleanup)

test('disabled GlassButton keeps pointer events so its tooltip still shows', () => {
  render(
    <GlassButton disabled title="busy — wait for the turn">
      New task
    </GlassButton>
  )
  const btn = screen.getByRole('button', { name: 'New task' }) as HTMLButtonElement
  assert.equal(btn.disabled, true)
  assert.equal(btn.title, 'busy — wait for the turn')
  assert.doesNotMatch(btn.className, /pointer-events-none/)
  assert.doesNotMatch(btn.className, /hover:/)
})

test('primary GlassButton swaps its focus ring off --ring, which matches its fill', () => {
  render(
    <>
      <GlassButton>Create</GlassButton>
      <GlassButton disabled>Save</GlassButton>
    </>
  )
  for (const name of ['Create', 'Save']) {
    assert.match(
      screen.getByRole('button', { name }).className,
      /focus-visible:outline-\[color:var\(--foreground\)\]/
    )
  }
})

test('GlassButton xs is the compact core size', () => {
  render(<GlassButton size="xs">back</GlassButton>)
  assert.match(screen.getByRole('button').className, /min-h-\[22px\]/)
})

test('IconButton is named, tooltipped and clickable', () => {
  let clicks = 0
  render(
    <IconButton aria-label="Close" onClick={() => clicks++}>
      <svg />
    </IconButton>
  )
  const btn = screen.getByRole('button', { name: 'Close' })
  assert.equal(btn.getAttribute('title'), 'Close')
  assert.equal(btn.getAttribute('type'), 'button')
  fireEvent.click(btn)
  assert.equal(clicks, 1)
})

test('Chip applies tone color, tint and border', () => {
  render(
    <Chip tone="error" data-testid="chip">
      failed
    </Chip>
  )
  const chip = screen.getByTestId('chip')
  assert.match(chip.className, /state-error/)
  assert.match(chip.className, /bg-state-error/)
  assert.match(chip.className, /border-state-error/)
})

test('outline Chip drops the tint', () => {
  render(
    <Chip tone="done" outline data-testid="chip">
      pass
    </Chip>
  )
  assert.doesNotMatch(screen.getByTestId('chip').className, /bg-state-done/)
})
