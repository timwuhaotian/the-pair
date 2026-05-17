import { test } from 'node:test'
import assert from 'node:assert/strict'
import { prependFileContext, selectReferencedFiles } from '../src/renderer/src/lib/fileMentions'

test('selectReferencedFiles only returns mentions still present in the spec', () => {
  const ctx = new Map([
    ['src/a.ts', 'a contents'],
    ['src/b.ts', 'b contents']
  ])
  const spec = 'Refactor @src/a.ts to use foo.'
  assert.deepEqual(selectReferencedFiles(spec, ctx), [['src/a.ts', 'a contents']])
})

test('prependFileContext returns spec unchanged when nothing referenced', () => {
  const spec = 'do the thing'
  assert.equal(prependFileContext(spec, new Map()), spec)
})

test('prependFileContext drops mentions the user deleted before submitting', () => {
  const ctx = new Map([
    ['src/keep.ts', 'KEEP'],
    ['src/dropped.ts', 'DROP']
  ])
  const spec = 'Update @src/keep.ts'
  const out = prependFileContext(spec, ctx)
  assert.ok(out.includes('@src/keep.ts:\nKEEP'))
  assert.ok(!out.includes('DROP'))
})

test('prependFileContext composes header + task block', () => {
  const ctx = new Map([['src/main.rs', 'fn main() {}']])
  const out = prependFileContext('Add a flag to @src/main.rs', ctx)
  assert.equal(
    out,
    '--- REFERENCED FILES ---\n@src/main.rs:\nfn main() {}\n\n--- TASK ---\nAdd a flag to @src/main.rs'
  )
})

test('prependFileContext concatenates multiple files with a blank line between them', () => {
  const ctx = new Map([
    ['a', 'AA'],
    ['b', 'BB']
  ])
  const out = prependFileContext('use @a and @b', ctx)
  assert.ok(out.includes('@a:\nAA\n\n@b:\nBB'))
})
