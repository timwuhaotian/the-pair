import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const css = await readFile(new URL('../src/renderer/src/assets/main.css', import.meta.url), 'utf8')
const mark = await readFile(
  new URL('../src/renderer/src/components/BrandMark.tsx', import.meta.url),
  'utf8'
)

/**
 * The brand artwork lives in BrandMark and its launch animation in the
 * stylesheet, joined only by class and keyframe names. A rename on one side
 * would silently drop the motion — nothing else would notice.
 */

test('every brand-mark rule in the stylesheet is rendered by the component', () => {
  const ruled = new Set([...css.matchAll(/\.(brand-mark[\w-]*)/g)].map((match) => match[1]))
  assert.ok(ruled.size > 0, 'no brand-mark rules found in main.css')
  for (const name of ruled) {
    assert.ok(mark.includes(name), `BrandMark.tsx never renders .${name}`)
  }
})

test('the animated parts of the mark all have a rule and a class', () => {
  for (const name of [
    'brand-mark--animated',
    'brand-mark__tile',
    'brand-mark__tile--mentor',
    'brand-mark__tile--executor',
    'brand-mark__check',
    'brand-mark__stick',
    'brand-mark__spark'
  ]) {
    assert.ok(css.includes(`.${name}`), `main.css has no rule for .${name}`)
    assert.ok(mark.includes(name), `BrandMark.tsx does not use .${name}`)
  }
})

test('every brand keyframe is both defined and played', () => {
  const defined = new Set([...css.matchAll(/@keyframes (brand-[\w-]+)/g)].map((match) => match[1]))
  const played = new Set(
    [...css.matchAll(/animation:[^;]*;/g)].flatMap((match) => match[0].match(/brand-[\w-]+/g) ?? [])
  )
  assert.ok(defined.size > 0, 'no brand keyframes found in main.css')
  assert.deepEqual([...played].sort(), [...defined].sort())
})

test('the previous robots-and-coffee artwork left no rules behind', () => {
  assert.doesNotMatch(css, /\.brand-mark__(bot|eyes|code|steam|cup)/)
  assert.doesNotMatch(css, /@keyframes hero-(bot|eye|code|steam)/)
})

test('reduced motion stills every animated part', () => {
  const reduced = css
    .split('@media (prefers-reduced-motion: reduce)')
    .slice(1)
    .find((block) => block.includes('.brand-mark--animated'))
  assert.ok(reduced, 'no reduced-motion block covers the brand mark')
  const block = reduced.slice(0, reduced.indexOf('animation: none'))
  for (const part of ['tile', 'check', 'stick', 'spark']) {
    assert.match(block, new RegExp(`\\.brand-mark--animated \\.brand-mark__${part}[,\\s]`))
  }
})

test('the check can be drawn: it has the unit path length the keyframes assume', () => {
  // brand-check-draw runs stroke-dashoffset from 1 to 0, which only spans the
  // whole stroke while the path declares pathLength 1.
  assert.match(css, /@keyframes brand-check-draw \{[^}]*stroke-dashoffset: 1;/)
  assert.match(mark, /className="brand-mark__check"[^>]*pathLength=\{1\}/)
})
