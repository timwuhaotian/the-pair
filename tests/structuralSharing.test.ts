import assert from 'node:assert/strict'
import test from 'node:test'
import { reuseById, reuseList, sameRecord } from '../src/renderer/src/lib/structuralSharing'

test('sameRecord treats undefined fields as absent and compares nested values', () => {
  assert.ok(sameRecord({ a: 1, b: undefined }, { a: 1 }))
  assert.ok(sameRecord({ u: { input: 3 } }, { u: { input: 3 } }))
  assert.ok(!sameRecord({ u: { input: 3 } }, { u: { input: 4 } }))
  assert.ok(!sameRecord({ a: 'x' }, { a: 'y' }))
})

test('reuseById returns the previous array when nothing changed', () => {
  const prev = [
    { id: 'a', content: 'one' },
    { id: 'b', content: 'two', usage: { input: 1 } }
  ]
  const next = [
    { id: 'a', content: 'one' },
    { id: 'b', content: 'two', usage: { input: 1 } }
  ]
  assert.ok(reuseById(prev, next) === prev)
})

test('reuseById keeps unchanged items and takes changed or new ones', () => {
  const a = { id: 'a', content: 'one' }
  const b = { id: 'b', content: 'two' }
  const out = reuseById(
    [a, b],
    [
      { id: 'a', content: 'one' },
      { id: 'b', content: 'two, edited' },
      { id: 'c', content: 'three' }
    ]
  )
  assert.equal(out.length, 3)
  assert.ok(out[0] === a, 'unchanged item keeps its identity')
  assert.ok(out[1] !== b)
  assert.equal(out[1].content, 'two, edited')
  assert.equal(out[2].id, 'c')
})

test('reuseById notices a reorder even when every item is reused', () => {
  const a = { id: 'a' }
  const b = { id: 'b' }
  const prev = [a, b]
  const out = reuseById(prev, [{ id: 'b' }, { id: 'a' }])
  assert.ok(out !== prev)
  assert.ok(out[0] === b && out[1] === a)
})

test('reuseList keeps the previous list only for an equal list', () => {
  const prev = [{ path: 'a.ts', status: 'M' }]
  assert.ok(reuseList(prev, [{ path: 'a.ts', status: 'M' }]) === prev)
  const changed = [{ path: 'a.ts', status: 'A' }]
  assert.ok(reuseList(prev, changed) === changed)
})
