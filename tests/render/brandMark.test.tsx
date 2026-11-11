/**
 * Component tests for the brand mark — the logo artwork shared by the boot
 * splash, the empty hub and the create-pair header.
 */
import './setup-dom.ts'
// eslint-disable-next-line @typescript-eslint/no-unused-vars
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { cleanup, render } from '@testing-library/react'
import { BrandMark } from '../../src/renderer/src/components/BrandMark.tsx'
import { assertAbsent } from './assert-dom.ts'

afterEach(cleanup)

test('the full mark draws the whole logo at the requested width', () => {
  const { container } = render(<BrandMark size={360} />)
  const svg = container.querySelector('svg') as SVGSVGElement
  assert.equal(svg.getAttribute('viewBox'), '0 0 720 456')
  assert.equal(svg.getAttribute('width'), '360')
  // The height follows the viewBox: 456/720 of the width.
  assert.equal(svg.getAttribute('height'), '228')
  // Stick and spark included.
  assert.ok(container.querySelector('.brand-mark__stick'))
  assert.ok(container.querySelector('.brand-mark__spark'))
})

test('the mark is a pair: a mentor tile and an executor tile, a check on each', () => {
  const { container } = render(<BrandMark />)
  const tiles = container.querySelectorAll('.brand-mark__tile')
  assert.equal(tiles.length, 2)
  assert.ok(tiles[0].classList.contains('brand-mark__tile--mentor'))
  assert.ok(tiles[1].classList.contains('brand-mark__tile--executor'))
  for (const tile of tiles) {
    assert.equal(tile.querySelectorAll('.brand-mark__check').length, 1)
    // CSS animates the tile group itself, which would replace a transform
    // attribute on it; the placement has to sit one level down.
    assert.equal(tile.getAttribute('transform'), null)
    assert.match(tile.firstElementChild?.getAttribute('transform') ?? '', /rotate\(-5 256 292\)/)
  }
  // Same tile, side by side: only the x offset differs.
  assert.notEqual(
    tiles[0].firstElementChild?.getAttribute('transform'),
    tiles[1].firstElementChild?.getAttribute('transform')
  )
})

test('the band mark crops to the two tiles', () => {
  const { container } = render(<BrandMark variant="band" size={624} />)
  const svg = container.querySelector('svg') as SVGSVGElement
  assert.equal(svg.getAttribute('viewBox'), '51 116 624 332')
  assert.equal(svg.getAttribute('height'), '332')
  assert.equal(container.querySelectorAll('.brand-mark__tile').length, 2)
  // Stick and spark lie outside the crop and are not drawn at all.
  assertAbsent(container.querySelector('.brand-mark__stick'), 'the band draws the stick')
  assertAbsent(container.querySelector('.brand-mark__spark'), 'the band draws the spark')
})

test('the mark is decorative and only animates when asked', () => {
  const { container, rerender } = render(<BrandMark size={80} />)
  const svg = container.querySelector('svg') as SVGSVGElement
  assert.equal(svg.getAttribute('aria-hidden'), 'true')
  assert.doesNotMatch(svg.className.baseVal, /brand-mark--animated/)

  rerender(<BrandMark size={80} animated />)
  const animated = container.querySelector('svg') as SVGSVGElement
  assert.match(animated.className.baseVal, /brand-mark--animated/)
})
