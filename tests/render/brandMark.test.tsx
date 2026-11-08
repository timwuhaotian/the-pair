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

afterEach(cleanup)

test('the full mark draws the whole logo at the requested width', () => {
  const { container } = render(<BrandMark size={160} />)
  const svg = container.querySelector('svg') as SVGSVGElement
  assert.equal(svg.getAttribute('viewBox'), '0 0 320 260')
  assert.equal(svg.getAttribute('width'), '160')
  // The height follows the viewBox: 260/320 of the width.
  assert.equal(svg.getAttribute('height'), '130')
  // Coffee cup included.
  assert.ok(container.querySelector('.brand-mark__cup'))
})

test('the band mark crops to the agent row', () => {
  const { container } = render(<BrandMark variant="band" size={128} />)
  const svg = container.querySelector('svg') as SVGSVGElement
  assert.equal(svg.getAttribute('viewBox'), '32 12 256 96')
  assert.equal(svg.getAttribute('height'), '48')
  assert.ok(container.querySelector('.brand-mark__bot--left'))
  assert.ok(container.querySelector('.brand-mark__bot--right'))
  assert.ok(container.querySelector('.brand-mark__code'))
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
