import assert from 'node:assert/strict'
import test from 'node:test'
import {
  Z,
  chipClass,
  errorCallout,
  fieldInput,
  menuItem,
  popoverSurface,
  toneClasses,
  type Tone,
  warningCallout
} from '../src/renderer/src/lib/ui.ts'

test('layers stack modal < popover < palette < toast < splash', () => {
  const order = [Z.modal, Z.popover, Z.palette, Z.toast, Z.splash]
  for (let i = 1; i < order.length; i++) {
    const prev = Number(/\d+/.exec(order[i - 1])?.[0])
    const next = Number(/\d+/.exec(order[i])?.[0])
    assert.ok(prev < next, `${order[i - 1]} should sit below ${order[i]}`)
  }
})

test('menu items share one geometry and differ only by highlight', () => {
  const idle = menuItem(false)
  const active = menuItem(true)
  assert.notEqual(idle, active)
  assert.match(active, /bg-foreground\/\[0\.08\]/)
  assert.match(idle, /hover:bg-foreground\/\[0\.05\]/)
  assert.match(idle, /transition-colors/)
})

test('popovers separate from the surface they open over', () => {
  // In dark mode --popover equals --surface-raised, so the strong rule is
  // the only thing that keeps a menu from melting into the modal below it.
  assert.match(popoverSurface, /border-rule-strong/)
  assert.match(popoverSurface, /bg-popover/)
})

test('text fields brighten their border on focus instead of an outline', () => {
  assert.match(fieldInput, /focus:outline-none/)
  assert.match(fieldInput, /focus:border-foreground\/50/)
  assert.match(fieldInput, /disabled:opacity-40/)
})

test('every tone maps to a foreground, tint and border', () => {
  const tones: Tone[] = ['neutral', 'mentor', 'executor', 'human', 'running', 'done', 'error']
  for (const tone of tones) {
    const c = toneClasses(tone)
    assert.ok(c.fg && c.bg && c.border, tone)
  }
  assert.equal(toneClasses('running').fg, 'state-running')
})

test('chips come in a pill (status) and a tag (stream metadata) shape', () => {
  assert.match(chipClass('pill'), /rounded-full/)
  assert.match(chipClass('tag'), /rounded-sm/)
  assert.match(chipClass('tag'), /uppercase/)
  assert.doesNotMatch(chipClass('pill'), /text-\[[89]px\]/)
  assert.doesNotMatch(chipClass('tag'), /text-\[[89]px\]/)
})

test('warning and error callouts share one geometry', () => {
  const strip = (s: string): string => s.replace(/state-(running|error)/g, 'state-x')
  assert.equal(strip(warningCallout), strip(errorCallout))
  assert.match(warningCallout, /border-l-2/)
})
