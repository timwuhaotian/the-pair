import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

const VARIANTS = ['a', 'b', 'c'] as const
const SIZES = [128, 512, 1024] as const

const MENTOR = '#2E7DE8'
const EXECUTOR = '#9B5DE5'

const assetUrl = (file: string) => new URL(`../docs/assets/logo-variants/${file}`, import.meta.url)

/** Reads width/height/colour-type straight out of the PNG IHDR chunk — no image deps needed. */
function readPngHeader(buffer: Buffer): { width: number; height: number; colorType: number } {
  const signature = buffer.subarray(0, 8).toString('hex')
  assert.equal(signature, '89504e470d0a1a0a', 'file is not a PNG')
  assert.equal(buffer.subarray(12, 16).toString('ascii'), 'IHDR', 'first chunk is not IHDR')
  return {
    width: buffer.readUInt32BE(16),
    height: buffer.readUInt32BE(20),
    colorType: buffer.readUInt8(25)
  }
}

for (const variant of VARIANTS) {
  test(`variant ${variant} SVG is a self-contained square master`, async () => {
    const svg = await readFile(assetUrl(`variant-${variant}.svg`), 'utf8')

    assert.match(svg, /viewBox="0 0 512 512"/, 'needs a square 512 viewBox')
    assert.match(svg, /width="512"/, 'needs an intrinsic width')
    assert.match(svg, /height="512"/, 'needs an intrinsic height')

    // A logo master must not depend on anything it does not carry itself.
    assert.doesNotMatch(svg, /<image\b/, 'must not embed a raster')
    assert.doesNotMatch(svg, /@font-face/, 'must not depend on a webfont')
    assert.doesNotMatch(svg, /font-family/, 'text must be outlined, not font-dependent')
    assert.doesNotMatch(svg, /href="(https?:)?\/\//, 'must not reference a remote resource')
  })

  test(`variant ${variant} SVG carries both agent colours and adapts to dark mode`, async () => {
    const svg = await readFile(assetUrl(`variant-${variant}.svg`), 'utf8')

    assert.ok(svg.includes(MENTOR), `mentor colour ${MENTOR} missing`)
    assert.ok(svg.includes(EXECUTOR), `executor colour ${EXECUTOR} missing`)
    assert.match(svg, /prefers-color-scheme: dark/, 'needs a dark-scheme override')
  })

  for (const size of SIZES) {
    test(`variant ${variant} renders a transparent ${size}px PNG`, async () => {
      const png = await readFile(assetUrl(`variant-${variant}-${size}.png`))
      const header = readPngHeader(png)

      assert.equal(header.width, size)
      assert.equal(header.height, size)
      // 4 = greyscale+alpha, 6 = RGBA. Anything else means we lost transparency.
      assert.ok(
        [4, 6].includes(header.colorType),
        `expected an alpha channel, got colour type ${header.colorType}`
      )
    })
  }
}
