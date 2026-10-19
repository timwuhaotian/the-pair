/**
 * Component tests for the `@file` popover placement: the side is picked from
 * the popover's rendered height, not its `max-h-64` cap, so a short list still
 * opens below a caret with modest room underneath (the New Task modal).
 */
import './setup-dom.ts'
import './mock-ipc.ts'
import React, { useRef } from 'react'
import assert from 'node:assert/strict'
import test, { afterEach, beforeEach } from 'node:test'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { FileMention } from '../../src/renderer/src/components/FileMention.tsx'

const FILES = [
  { path: 'src/app.ts', type: 'file' as const },
  { path: 'src/main.ts', type: 'file' as const },
  { path: 'README.md', type: 'file' as const }
]

// jsdom's window is 768px tall; the caret line sits at 600–618, leaving 172px
// below it and 622px above.
const CARET_RECT = { top: 600, bottom: 618, left: 20, right: 32, width: 12, height: 18 }

let popoverHeight = 0
const originalRect = window.Element.prototype.getBoundingClientRect
const originalListFiles = window.api.file.listFiles

function Harness(): React.ReactNode {
  const ref = useRef<HTMLTextAreaElement>(null)
  return (
    <>
      <textarea ref={ref} data-testid="spec" />
      <FileMention textareaRef={ref} onChange={() => undefined} directory="/repo" />
    </>
  )
}

async function openPopover(): Promise<HTMLElement> {
  render(<Harness />)
  const textarea = screen.getByTestId('spec')
  textarea.focus()
  fireEvent.input(textarea, { target: { value: '@' } })
  const item = await screen.findByText('src/app.ts')
  const popover = item.closest<HTMLElement>('.fixed')
  assert.ok(popover, 'popover rendered')
  return popover
}

beforeEach(() => {
  window.api.file.listFiles = async () => FILES
  window.Element.prototype.getBoundingClientRect = () => ({
    ...CARET_RECT,
    x: CARET_RECT.left,
    y: CARET_RECT.top,
    toJSON: () => CARET_RECT
  })
  Object.defineProperty(window.HTMLElement.prototype, 'offsetHeight', {
    configurable: true,
    get: () => popoverHeight
  })
})

afterEach(() => {
  cleanup()
  window.api.file.listFiles = originalListFiles
  window.Element.prototype.getBoundingClientRect = originalRect
  delete (window.HTMLElement.prototype as { offsetHeight?: number }).offsetHeight
})

test('a short @file list opens below the caret when it fits there', async () => {
  popoverHeight = 79
  const popover = await openPopover()
  assert.equal(popover.style.top, '596px')
  assert.equal(popover.style.bottom, '')
})

test('a tall @file list flips above the caret when it would not fit below', async () => {
  popoverHeight = 256
  const popover = await openPopover()
  assert.equal(popover.style.bottom, '146px')
  assert.equal(popover.style.top, '')
})
