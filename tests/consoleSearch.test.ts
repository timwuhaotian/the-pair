import assert from 'node:assert/strict'
import test from 'node:test'

import {
  countOccurrences,
  findMatchRanges,
  nextMatchIndex
} from '../src/renderer/src/lib/consoleSearch.ts'

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

test('findMatchRanges collapses whitespace runs and soft breaks like the rendered text', () => {
  // Normal-flow markdown collapses runs of spaces and soft line breaks to a
  // single visible space, so the query the reader types off the card must
  // match the collapsed text. The range spans the full raw whitespace it
  // replaces, so <mark> wraps the whole rendered phrase.
  const spaced = findMatchRanges('The parser  was   correct overall', 'parser was correct')
  assert.deepEqual(spaced, [[4, 25]])
  assert.equal(
    'The parser  was   correct overall'.slice(spaced[0]![0], spaced[0]![1]),
    'parser  was   correct'
  )

  const soft = findMatchRanges('parser\nwas correct', 'parser was correct')
  assert.deepEqual(soft, [[0, 18]])
  assert.equal('parser\nwas correct'.slice(soft[0]![0], soft[0]![1]), 'parser\nwas correct')

  // Whitespace collapses on the query side too, both ways.
  assert.deepEqual(findMatchRanges('parser was correct', 'parser   was'), [[0, 10]])
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
