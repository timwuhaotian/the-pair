import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

const readme = await readFile(new URL('../README.md', import.meta.url), 'utf8')

test('README presents Pair Code as the terminal edition of The Pair', () => {
  assert.match(readme, /\*\*CLI\*\*.*Pair Code/s)
  assert.match(readme, /https:\/\/github\.com\/timwuhaotian\/pair-code/)
  assert.match(readme, /https:\/\/www\.npmjs\.com\/package\/pair-code/)
  assert.match(readme, /npm install -g pair-code/)
})
