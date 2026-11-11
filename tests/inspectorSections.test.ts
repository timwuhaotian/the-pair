import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const panel = await readFile(
  new URL('../src/renderer/src/components/PairOperationsPanel.tsx', import.meta.url),
  'utf8'
)

const section = await readFile(
  new URL('../src/renderer/src/components/inspector/InspectorSection.tsx', import.meta.url),
  'utf8'
)

const inlineDiff = await readFile(
  new URL('../src/renderer/src/components/inspector/InlineDiff.tsx', import.meta.url),
  'utf8'
)

const history = await readFile(
  new URL('../src/renderer/src/components/TaskHistoryPanel.tsx', import.meta.url),
  'utf8'
)

const timeline = await readFile(
  new URL('../src/renderer/src/components/TimelinePanel.tsx', import.meta.url),
  'utf8'
)

const banner = await readFile(
  new URL('../src/renderer/src/components/RecoveryBanner.tsx', import.meta.url),
  'utf8'
)

const hub = await readFile(
  new URL('../src/renderer/src/components/PairsHub.tsx', import.meta.url),
  'utf8'
)

test('InspectorSection exports a collapsible section primitive', () => {
  assert.match(section, /export function InspectorSection\(/)
  assert.match(section, /aria-expanded/)
  assert.match(section, /inspector-toggle-/)
  assert.match(section, /inspector-section-/)
})

test('inspector groups models, run state, usage, files, history, timeline', () => {
  for (const id of ['models', 'run-state', 'usage', 'files', 'history', 'timeline']) {
    assert.match(panel, new RegExp(`id="${id}"`), `missing section ${id}`)
  }
  assert.match(panel, /<InspectorSection/)
})

test('modified files expand inline diffs instead of opening a modal', () => {
  assert.match(panel, /ops-file-row-/)
  assert.match(panel, /ops-file-diff-/)
  assert.match(panel, /<InlineDiff/)
  assert.match(inlineDiff, /export function InlineDiff\(/)
  assert.doesNotMatch(panel, /FileDiffModal/)
})

test('TaskHistoryPanel and TimelinePanel support bare embedding', () => {
  assert.match(history, /bare/)
  assert.match(timeline, /export function TimelineActions\(/)
  assert.match(timeline, /bare/)
  assert.match(panel, /<TaskHistoryPanel[\s\S]*?bare/)
  assert.match(panel, /<TimelinePanel timeline=\{timelineData\} bare/)
})

test('hub surfaces recoverable pairs as a dismissible banner', () => {
  assert.match(banner, /export function RecoveryBanner\(/)
  assert.match(banner, /recovery-banner/)
  assert.match(banner, /recovery-dismiss/)
  assert.match(hub, /<RecoveryBanner/)
  assert.match(hub, /recoverablePairs/)
})

test('recovery copy is localized', async () => {
  const en = JSON.parse(
    await readFile(new URL('../src/renderer/src/locales/en.json', import.meta.url), 'utf8')
  )
  assert.equal(typeof en.recovery.title, 'string')
  assert.equal(typeof en.recovery.description, 'string')
  assert.equal(typeof en.common.dismiss, 'string')
})
