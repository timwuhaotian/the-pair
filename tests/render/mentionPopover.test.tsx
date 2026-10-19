/**
 * Component tests for the `@file` popover:
 *   - placement: the side is picked from the popover's rendered height, not
 *     its `max-h-72` cap, so a short list still opens below a caret with modest
 *     room underneath (the New Task modal)
 *   - when it opens: only on an `@` that starts a token, never inside an email
 *   - the empty hint tells a loading or failed listing apart from an empty repo
 */
import './setup-dom.ts'
import './mock-ipc.ts'
import React, { useRef } from 'react'
import assert from 'node:assert/strict'
import test, { afterEach, beforeEach } from 'node:test'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
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

function typeInto(value: string): void {
  const textarea = screen.getByTestId('spec') as HTMLTextAreaElement
  textarea.focus()
  fireEvent.input(textarea, { target: { value } })
}

async function openPopover(): Promise<HTMLElement> {
  render(<Harness />)
  typeInto('@')
  const item = await screen.findByText('src/app.ts')
  const popover = item.closest<HTMLElement>('.fixed')
  assert.ok(popover, 'popover rendered')
  return popover
}

beforeEach(() => {
  void i18n.changeLanguage('en')
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

test('an @ inside an email address does not open the popover', async () => {
  render(<Harness />)
  typeInto('ping me at user@example.com')
  // Let the file listing settle so a late render would show up.
  await new Promise((resolve) => setTimeout(resolve, 0))
  // Boolean asserts: a failing equal() on a jsdom node inspects it for minutes.
  assert.ok(!screen.queryByRole('listbox'), 'popover stays closed')
  typeInto('ping me at user@example.com @src')
  assert.ok(await screen.findByRole('listbox'))
})

test('the popover says files are loading until the listing resolves', async () => {
  let resolveList: (files: typeof FILES) => void = () => undefined
  window.api.file.listFiles = () => new Promise((resolve) => (resolveList = resolve))
  render(<Harness />)
  typeInto('@')
  assert.ok(await screen.findByText('loading files…'))
  resolveList(FILES)
  assert.ok(await screen.findByText('src/app.ts'))
  assert.ok(!screen.queryByText('loading files…'), 'loading hint is gone')
})

test('a failed listing reads as a failure, not an empty repo', async () => {
  const originalWarn = console.warn
  console.warn = () => undefined
  try {
    window.api.file.listFiles = async () => {
      throw new Error('permission denied')
    }
    render(<Harness />)
    typeInto('@')
    assert.ok(await screen.findByText("couldn't list files"))
  } finally {
    console.warn = originalWarn
  }
})

test('an empty repo says no files were found', async () => {
  window.api.file.listFiles = async () => []
  render(<Harness />)
  typeInto('@')
  assert.ok(await screen.findByText('no files found'))
})
