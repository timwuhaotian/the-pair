import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const source = await readFile(
  new URL('../src/renderer/src/components/QuickActionsFAB.tsx', import.meta.url),
  'utf8'
)

const app = await readFile(new URL('../src/renderer/src/App.tsx', import.meta.url), 'utf8')

test('QuickActionsFAB only exposes wired dashboard actions', () => {
  assert.match(source, /onAction\('create'\)/)
  assert.doesNotMatch(source, /id: 'settings'/)
  assert.doesNotMatch(source, /id: 'help'/)
  assert.doesNotMatch(source, /id: 'shortcuts'/)
})

test('QuickActionsFAB invokes create directly instead of opening a dead menu', () => {
  assert.match(source, /onAction\('create'\)/)
  assert.doesNotMatch(source, /setIsOpen/)
})

test('dashboard no longer mounts the floating quick action create button', () => {
  assert.doesNotMatch(app, /<QuickActionsFAB/)
})
