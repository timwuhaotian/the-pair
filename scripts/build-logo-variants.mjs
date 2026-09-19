#!/usr/bin/env node
import { execFileSync } from 'child_process'
import { existsSync, mkdtempSync, rmSync } from 'fs'
import { tmpdir } from 'os'
import { fileURLToPath } from 'url'
import { dirname, join, resolve } from 'path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const rootDir = join(__dirname, '..')

/** Candidate logo marks under review. Shared with tests/logoVariants.test.ts. */
export const VARIANTS = [
  { key: 'a', label: 'A - Evolved Pair' },
  { key: 'b', label: 'B - Handoff Loop' },
  { key: 'c', label: 'C - Prompt' }
]

/** Raster sizes emitted per variant: favicon/README, app icon, store artwork. */
export const SIZES = [128, 512, 1024]

/** Sizes shown side by side on the contact sheet, to judge small-size legibility. */
const PROOF_SIZES = [16, 32, 64, 128]

const THEMES = [
  { name: 'light', bg: '#FAFAF7', ink: '#1A2332' },
  { name: 'dark', bg: '#0A0E14', ink: '#E6EDF3' }
]

const COL_W = 600
const BIG_H = 360
const CELL = 150
const LABEL_H = 50
const TITLE_H = 66

export const VARIANT_DIR = join('docs', 'assets', 'logo-variants')
export const svgName = (variant) => `variant-${variant}.svg`
export const pngName = (variant, size) => `variant-${variant}-${size}.png`

const dir = join(rootDir, VARIANT_DIR)
const magick = (args) => execFileSync('magick', args.map(String))

function findFont() {
  const candidates = [
    '/System/Library/Fonts/SFNSMono.ttf',
    '/System/Library/Fonts/Menlo.ttc',
    '/System/Library/Fonts/Helvetica.ttc'
  ]
  return candidates.find((f) => existsSync(f)) ?? null
}

function rasterize() {
  let written = 0
  for (const { key } of VARIANTS) {
    const svg = join(dir, svgName(key))
    if (!existsSync(svg)) {
      console.error(`✗ missing source: ${VARIANT_DIR}/${svgName(key)}`)
      process.exit(1)
    }
    for (const size of SIZES) {
      const out = join(dir, pngName(key, size))
      // rsvg-convert keeps the alpha channel, so marks stay usable on either theme.
      execFileSync('rsvg-convert', ['-w', String(size), '-h', String(size), '-o', out, svg])
      console.log(`  → ${VARIANT_DIR}/${pngName(key, size)}`)
      written += 1
    }
  }
  console.log(`✓ rasterized ${written} PNGs from ${VARIANTS.length} variants\n`)
}

/**
 * Contact sheet: one column per variant, each showing the mark large plus a
 * 16/32/64/128 proof row, on both the light and the dark app background.
 * Every band is built at an exact size and stacked, so nothing relies on
 * gravity maths lining up across composites.
 */
function buildContactSheet(font) {
  const tmp = mkdtempSync(join(tmpdir(), 'logo-sheet-'))
  const t = (name) => join(tmp, name)

  try {
    const columns = []

    for (const { key, label } of VARIANTS) {
      const svg = join(dir, svgName(key))
      for (const size of PROOF_SIZES) {
        execFileSync('rsvg-convert', ['-w', String(size), '-h', String(size), '-o', t(`${key}-${size}.png`), svg])
      }

      const bands = []

      // title band, always on the light background so columns read as one header row
      const title = t(`${key}-title.png`)
      const titleArgs = ['-size', `${COL_W}x${TITLE_H}`, `xc:${THEMES[0].bg}`]
      if (font) titleArgs.push('-font', font, '-fill', THEMES[0].ink, '-pointsize', '32', '-gravity', 'center', '-annotate', '+0+0', label)
      magick([...titleArgs, title])
      bands.push(title)

      for (const theme of THEMES) {
        const big = t(`${key}-${theme.name}-big.png`)
        magick([join(dir, pngName(key, 512)), '-resize', '300x300', '-background', theme.bg, '-gravity', 'center', '-extent', `${COL_W}x${BIG_H}`, big])
        bands.push(big)

        // proof row: one fixed-width cell per size, so labels align under their marks
        const cells = []
        const labels = []
        for (const size of PROOF_SIZES) {
          const cell = t(`${key}-${theme.name}-cell-${size}.png`)
          magick([t(`${key}-${size}.png`), '-background', theme.bg, '-gravity', 'center', '-extent', `${CELL}x${CELL}`, cell])
          cells.push(cell)

          const lbl = t(`${key}-${theme.name}-lbl-${size}.png`)
          const lblArgs = ['-size', `${CELL}x${LABEL_H}`, `xc:${theme.bg}`]
          if (font) lblArgs.push('-font', font, '-fill', theme.ink, '-pointsize', '22', '-gravity', 'center', '-annotate', '+0+0', String(size))
          magick([...lblArgs, lbl])
          labels.push(lbl)
        }

        const strip = t(`${key}-${theme.name}-strip.png`)
        magick([...cells, '+append', '-background', theme.bg, '-gravity', 'center', '-extent', `${COL_W}x${CELL}`, strip])
        bands.push(strip)

        const labelRow = t(`${key}-${theme.name}-labels.png`)
        magick([...labels, '+append', '-background', theme.bg, '-gravity', 'center', '-extent', `${COL_W}x${LABEL_H}`, labelRow])
        bands.push(labelRow)
      }

      const column = t(`col-${key}.png`)
      magick([...bands, '-append', column])
      columns.push(column)
    }

    const sheet = join(dir, 'contact-sheet.png')
    magick([...columns, '+append', '-bordercolor', '#C9C9C4', '-border', '1', sheet])
    console.log(`✓ ${VARIANT_DIR}/contact-sheet.png`)
  } finally {
    rmSync(tmp, { recursive: true, force: true })
  }
}

function requireTool(bin, hint) {
  try {
    execFileSync(bin, ['--version'], { stdio: 'ignore' })
  } catch {
    console.error(`✗ ${bin} not found. Install it with: ${hint}`)
    process.exit(1)
  }
}

const invokedDirectly =
  process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (invokedDirectly) {
  requireTool('rsvg-convert', 'brew install librsvg')
  requireTool('magick', 'brew install imagemagick')

  const font = findFont()
  if (!font) console.warn('⚠ no system font found — contact sheet will be unlabelled')

  rasterize()
  buildContactSheet(font)
}
