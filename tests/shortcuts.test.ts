import { describe, it } from 'node:test'
import assert from 'node:assert'
import {
  matchesShortcut,
  formatShortcutLabel,
  formatShortcutParts,
  type ShortcutDef
} from '../src/renderer/src/lib/shortcuts.ts'

function makeKeyEvent(
  key: string,
  opts: { metaKey?: boolean; ctrlKey?: boolean; shiftKey?: boolean; altKey?: boolean }
): KeyboardEvent {
  return {
    key,
    metaKey: opts.metaKey ?? false,
    ctrlKey: opts.ctrlKey ?? false,
    shiftKey: opts.shiftKey ?? false,
    altKey: opts.altKey ?? false
  } as unknown as KeyboardEvent
}

describe('matchesShortcut', () => {
  it('matches simple key without modifiers', () => {
    const shortcut: ShortcutDef = {
      key: 'Escape',
      modifiers: [],
      handler: () => {},
      description: 'test'
    }
    const event = makeKeyEvent('Escape', {})
    assert.strictEqual(matchesShortcut(event, shortcut), true)
  })

  it('matches Cmd+P', () => {
    const shortcut: ShortcutDef = {
      key: 'p',
      modifiers: ['meta'],
      handler: () => {},
      description: 'test'
    }
    const event = makeKeyEvent('p', { metaKey: true })
    assert.strictEqual(matchesShortcut(event, shortcut), true)
  })

  it('does not match if extra modifier pressed', () => {
    const shortcut: ShortcutDef = {
      key: 'p',
      modifiers: ['meta'],
      handler: () => {},
      description: 'test'
    }
    const event = makeKeyEvent('p', { metaKey: true, shiftKey: true })
    assert.strictEqual(matchesShortcut(event, shortcut), false)
  })

  it('does not match wrong key', () => {
    const shortcut: ShortcutDef = {
      key: 'p',
      modifiers: ['meta'],
      handler: () => {},
      description: 'test'
    }
    const event = makeKeyEvent('n', { metaKey: true })
    assert.strictEqual(matchesShortcut(event, shortcut), false)
  })

  it('matches Ctrl+Shift+P', () => {
    const shortcut: ShortcutDef = {
      key: 'p',
      modifiers: ['ctrl', 'shift'],
      handler: () => {},
      description: 'test'
    }
    const event = makeKeyEvent('P', { ctrlKey: true, shiftKey: true })
    assert.strictEqual(matchesShortcut(event, shortcut), true)
  })

  it('case-insensitive key matching', () => {
    const shortcut: ShortcutDef = {
      key: 'n',
      modifiers: ['meta'],
      handler: () => {},
      description: 'test'
    }
    const event = makeKeyEvent('N', { metaKey: true })
    assert.strictEqual(matchesShortcut(event, shortcut), true)
  })
})

describe('formatShortcutLabel', () => {
  it('formats Cmd+P', () => {
    const shortcut: ShortcutDef = {
      key: 'p',
      modifiers: ['meta'],
      handler: () => {},
      description: 'test'
    }
    const label = formatShortcutLabel(shortcut)
    assert.ok(label.includes('P'))
  })

  it('formats Cmd+Shift+P', () => {
    const shortcut: ShortcutDef = {
      key: 'p',
      modifiers: ['meta', 'shift'],
      handler: () => {},
      description: 'test'
    }
    const label = formatShortcutLabel(shortcut)
    assert.ok(label.includes('P'))
  })

  it('drops the redundant Shift token for symbol keys like "?"', () => {
    const shortcut: ShortcutDef = {
      key: '?',
      modifiers: ['shift'],
      handler: () => {},
      description: 'test'
    }
    // "?" already implies Shift, so the label should be just "?" — no ⇧/Shift prefix.
    assert.strictEqual(formatShortcutLabel(shortcut), '?')
    assert.deepStrictEqual(formatShortcutParts(shortcut), ['?'])
  })

  it('keeps the Shift token for alphanumeric keys', () => {
    const shortcut: ShortcutDef = {
      key: 'p',
      modifiers: ['shift'],
      handler: () => {},
      description: 'test'
    }
    const parts = formatShortcutParts(shortcut)
    assert.strictEqual(parts.length, 2)
    assert.strictEqual(parts[parts.length - 1], 'P')
  })

  it('joins parts into the same string formatShortcutLabel returns', () => {
    const shortcut: ShortcutDef = {
      key: 'n',
      modifiers: ['meta'],
      handler: () => {},
      description: 'test'
    }
    assert.strictEqual(formatShortcutParts(shortcut).join(''), formatShortcutLabel(shortcut))
  })
})
