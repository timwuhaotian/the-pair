import assert from 'node:assert/strict'
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'

import { buildManifest } from '../scripts/generate-updater-manifest.mjs'

const assets = [
  'the-pair-1.1.12.app.tar.gz',
  'the-pair-1.1.12-setup.exe',
  'the-pair-1.1.12.AppImage.tar.gz'
]

async function makeDist() {
  const root = await mkdtemp(join(tmpdir(), 'the-pair-manifest-'))
  const distDir = join(root, 'dist')
  await mkdir(distDir, { recursive: true })
  for (const asset of assets) {
    await writeFile(join(distDir, asset), 'artifact')
    await writeFile(join(distDir, `${asset}.sig`), 'signature')
  }
  return { root, distDir }
}

test('buildManifest produces a Tauri updater manifest with all platform entries', async () => {
  const { root, distDir } = await makeDist()
  await writeFile(join(distDir, 'the-pair-1.1.12.app.tar.gz.sig'), 'mac-sig')
  await writeFile(join(distDir, 'the-pair-1.1.12-setup.exe.sig'), 'win-sig')
  await writeFile(join(distDir, 'the-pair-1.1.12.AppImage.tar.gz.sig'), 'linux-sig')
  const notesPath = join(root, 'release-notes.md')
  await writeFile(notesPath, 'Release notes go here')

  const manifest = buildManifest({
    version: '1.1.12',
    baseUrl: 'https://github.com/example/the-pair/releases/download/v1.1.12',
    distDir,
    notesPath,
    pubDate: '2026-03-25T00:00:00.000Z'
  })

  assert.equal(manifest.version, '1.1.12')
  assert.equal(manifest.pub_date, '2026-03-25T00:00:00.000Z')
  assert.equal(manifest.notes, 'Release notes go here')
  assert.equal(
    manifest.platforms['darwin-x86_64'].url,
    'https://github.com/example/the-pair/releases/download/v1.1.12/the-pair-1.1.12.app.tar.gz'
  )
  assert.equal(manifest.platforms['darwin-aarch64'].signature, 'mac-sig')
  assert.equal(
    manifest.platforms['windows-x86_64'].url,
    'https://github.com/example/the-pair/releases/download/v1.1.12/the-pair-1.1.12-setup.exe'
  )
  assert.equal(manifest.platforms['windows-x86_64'].signature, 'win-sig')
  assert.equal(manifest.platforms['linux-x86_64'].signature, 'linux-sig')
})

test('buildManifest rejects missing, empty and non-file assets and signatures on every platform', async () => {
  for (const asset of assets) {
    for (const suffix of ['', '.sig']) {
      for (const invalid of ['missing', 'empty', 'directory']) {
        const { distDir } = await makeDist()
        const path = join(distDir, `${asset}${suffix}`)
        await rm(path)
        if (invalid === 'empty') await writeFile(path, suffix ? ' \n\t' : '')
        if (invalid === 'directory') await mkdir(path)
        assert.throws(
          () => buildManifest({ version: '1.1.12', baseUrl: 'https://example.com', distDir }),
          suffix ? /Missing or empty updater signature/ : /Missing or empty updater asset/
        )
      }
    }
  }
})
