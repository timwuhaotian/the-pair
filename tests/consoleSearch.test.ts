import assert from 'node:assert/strict'
import test from 'node:test'

import {
  countOccurrences,
  filterMessagesByQuery,
  findMatchRanges,
  nextMatchIndex
} from '../src/renderer/src/lib/consoleSearch.ts'

const msgs = [
  { id: 'a', content: 'Fix the Parser bug' },
  { id: 'b', content: 'nothing relevant' },
  { id: 'c', content: 'parser PARSER Parser' }
]

test('filterMessagesByQuery matches case-insensitive substring on content', () => {
  assert.deepEqual(
    filterMessagesByQuery(msgs, 'parser').map((m) => m.id),
    ['a', 'c']
  )
})

test('filterMessagesByQuery trims the query', () => {
  assert.deepEqual(
    filterMessagesByQuery(msgs, '  BUG ').map((m) => m.id),
    ['a']
  )
})

test('filterMessagesByQuery returns the same reference for an empty query', () => {
  assert.equal(filterMessagesByQuery(msgs, ''), msgs)
  assert.equal(filterMessagesByQuery(msgs, '   '), msgs)
})

test('filterMessagesByQuery returns empty when nothing matches', () => {
  assert.deepEqual(filterMessagesByQuery(msgs, 'zzz'), [])
})

test('findMatchRanges returns all non-overlapping ranges, case-insensitive', () => {
  assert.deepEqual(findMatchRanges('parser PARSER Parser', 'parser'), [
    [0, 6],
    [7, 13],
    [14, 20]
  ])
})

test('findMatchRanges does not overlap matches', () => {
  assert.deepEqual(findMatchRanges('aaaa', 'aa'), [
    [0, 2],
    [2, 4]
  ])
  assert.deepEqual(findMatchRanges('aaa', 'aa'), [[0, 2]])
})

test('findMatchRanges returns [] for empty query or no match', () => {
  assert.deepEqual(findMatchRanges('abc', ''), [])
  assert.deepEqual(findMatchRanges('abc', '  '), [])
  assert.deepEqual(findMatchRanges('abc', 'z'), [])
  assert.deepEqual(findMatchRanges('', 'a'), [])
})

test('findMatchRanges treats query literally, not as regex', () => {
  assert.deepEqual(findMatchRanges('a.c aqc', 'a.c'), [[0, 3]])
})

test('findMatchRanges keeps offsets aligned when lowercasing changes length', () => {
  // U+0130 lowercases to two code units; ranges must index the original string.
  const text = 'İİ parser'
  const ranges = findMatchRanges(text, 'parser')
  assert.deepEqual(ranges, [[3, 9]])
  assert.equal(text.slice(ranges[0]![0], ranges[0]![1]), 'parser')
})

test('findMatchRanges matches a query whose lowercase form changes length', () => {
  const text = 'a İ b'
  const ranges = findMatchRanges(text, 'İ')
  assert.equal(ranges.length, 1)
  assert.equal(text.slice(ranges[0]![0], ranges[0]![1]), 'İ')
})

test('filterMessagesByQuery uses the same matching as findMatchRanges', () => {
  const list = [{ id: 'u', content: 'İİ parser' }]
  assert.deepEqual(
    filterMessagesByQuery(list, 'parser').map((m) => m.id),
    ['u']
  )
})

test('countOccurrences counts matches', () => {
  assert.equal(countOccurrences('parser PARSER', 'parser'), 2)
  assert.equal(countOccurrences('abc', ''), 0)
  assert.equal(countOccurrences('abc', 'z'), 0)
})

test('nextMatchIndex wraps forward and backward', () => {
  assert.equal(nextMatchIndex(-1, 3, 1), 0)
  assert.equal(nextMatchIndex(0, 3, 1), 1)
  assert.equal(nextMatchIndex(2, 3, 1), 0)
  assert.equal(nextMatchIndex(0, 3, -1), 2)
  assert.equal(nextMatchIndex(-1, 3, -1), 2)
})

test('nextMatchIndex returns -1 when there are no matches', () => {
  assert.equal(nextMatchIndex(-1, 0, 1), -1)
  assert.equal(nextMatchIndex(0, 0, -1), -1)
})
