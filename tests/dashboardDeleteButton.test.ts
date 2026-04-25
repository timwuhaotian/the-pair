import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

test('pair card delete button is always visible', async () => {
  const source = await readFile('src/renderer/src/components/Dashboard.tsx', 'utf8')
  const deleteButtonStart = source.indexOf('variant="destructive"')
  const deleteButtonEnd = source.indexOf('</GlassButton>', deleteButtonStart)
  const deleteButtonSource = source.slice(deleteButtonStart, deleteButtonEnd)

  assert.ok(deleteButtonStart > -1, 'delete button should exist')
  assert.doesNotMatch(deleteButtonSource, /opacity-0/)
  assert.doesNotMatch(deleteButtonSource, /group-hover:opacity-100/)
})
