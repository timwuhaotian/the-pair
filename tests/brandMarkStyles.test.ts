import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const css = await readFile(new URL('../src/renderer/src/assets/main.css', import.meta.url), 'utf8')
const mark = await readFile(
  new URL('../src/renderer/src/components/BrandMark.tsx', import.meta.url),
  'utf8'
)

/**
 * The brand artwork moved out of StartupHero into BrandMark, and its CSS
 * classes with it. A rename that leaves the stylesheet behind would silently
 * drop the float, blink, steam and glow — nothing else would notice.
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
    'brand-mark__art',
    'brand-mark--animated',
    'brand-mark__bot--left',
    'brand-mark__bot--right',
    'brand-mark__code',
    'brand-mark__eyes--left',
    'brand-mark__eyes--right',
    'brand-mark__steam-1',
    'brand-mark__steam-2',
    'brand-mark__steam-3'
  ]) {
    assert.ok(css.includes(`.${name}`), `main.css has no rule for .${name}`)
    assert.ok(mark.includes(name), `BrandMark.tsx does not use .${name}`)
  }
})

test('the old startup-hero artwork rules are gone', () => {
  assert.doesNotMatch(css, /\.startup-hero__art/)
  assert.doesNotMatch(css, /\.startup-hero--animated \.startup-hero__(bot|code|steam|eyes)/)
})

test('the animated art rule matches the element that carries both classes', () => {
  // The art and the animated flag live on the same <svg>, so the rule has to
  // be a compound selector — a descendant one would never match.
  assert.match(css, /\.brand-mark--animated\.brand-mark__art/)
  assert.match(mark, /'brand-mark__art[^']*'/)
})
