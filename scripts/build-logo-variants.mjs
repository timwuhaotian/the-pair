#!/usr/bin/env node
import { execFileSync } from 'child_process'
import { existsSync } from 'fs'
import { fileURLToPath } from 'url'
import { dirname, join, resolve } from 'path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const rootDir = join(__dirname, '..')

/** Candidate logo marks under review. Shared with tests/logoVariants.test.ts. */
export const VARIANTS = ['a', 'b', 'c']

/** Raster sizes emitted per variant: favicon/README, app icon, store artwork. */
export const SIZES = [128, 512, 1024]

export const VARIANT_DIR = join('docs', 'assets', 'logo-variants')
export const svgName = (variant) => `variant-${variant}.svg`
export const pngName = (variant, size) => `variant-${variant}-${size}.png`

function rasterize() {
  const dir = join(rootDir, VARIANT_DIR)
  let written = 0

  for (const variant of VARIANTS) {
    const svg = join(dir, svgName(variant))
    if (!existsSync(svg)) {
      console.error(`✗ missing source: ${VARIANT_DIR}/${svgName(variant)}`)
      process.exit(1)
    }

    for (const size of SIZES) {
      const out = join(dir, pngName(variant, size))
      // rsvg-convert keeps the alpha channel, so marks stay usable on either theme.
      execFileSync('rsvg-convert', ['-w', String(size), '-h', String(size), '-o', out, svg])
      console.log(`  → ${VARIANT_DIR}/${pngName(variant, size)}`)
      written += 1
    }
  }

  console.log(`\n✓ rasterized ${written} PNGs from ${VARIANTS.length} variants`)
}

const invokedDirectly = process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (invokedDirectly) {
  try {
    execFileSync('rsvg-convert', ['--version'], { stdio: 'ignore' })
  } catch {
    console.error('✗ rsvg-convert not found. Install it with: brew install librsvg')
    process.exit(1)
  }
  rasterize()
}
