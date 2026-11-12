import assert from 'node:assert/strict'
import { mkdtemp, mkdir, readFile, readdir, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'

import { collectUpdaterAssets } from '../scripts/release-updater-assets.mjs'

async function createTempFile(relativePath: string, contents = 'artifact') {
  const root = await mkdtemp(join(tmpdir(), 'the-pair-updater-assets-'))
  const filePath = join(root, relativePath)
  await mkdir(join(filePath, '..'), { recursive: true })
  await writeFile(filePath, contents)
  return { root, filePath }
}

test('collectUpdaterAssets rejects unsupported platforms and missing bundle directories', async () => {
  const root = await mkdtemp(join(tmpdir(), 'the-pair-updater-assets-'))
  const options = {
    platform: 'unknown',
    bundleRoot: root,
    outDir: join(root, 'dist'),
    version: '1'
  }
  assert.throws(() => collectUpdaterAssets(options), /Unsupported platform/)
  assert.throws(
    () => collectUpdaterAssets({ ...options, platform: 'macos' }),
    /Expected bundle directory/
  )
})

test('collectUpdaterAssets copies macOS updater archives and signatures', async () => {
  const { root } = await createTempFile('macos/The Pair.app.tar.gz', 'mac-update')
  await writeFile(join(root, 'macos', 'The Pair.app.tar.gz.sig'), 'mac-sig')

  const outDir = join(root, 'dist')
  const result = collectUpdaterAssets({
    platform: 'macos',
    bundleRoot: root,
    outDir,
    version: '1.1.12'
  })

  assert.equal(result.length, 2)
  assert.equal(await readFile(join(outDir, 'the-pair-1.1.12.app.tar.gz'), 'utf8'), 'mac-update')
  assert.equal(await readFile(join(outDir, 'the-pair-1.1.12.app.tar.gz.sig'), 'utf8'), 'mac-sig')
})

test('collectUpdaterAssets rejects missing, empty, mismatched, or ambiguous files before copying', async () => {
  const { root, filePath } = await createTempFile('macos/nested/The Pair.app.tar.gz')
  const outDir = join(root, 'dist')
  const options = { platform: 'macos', bundleRoot: root, outDir, version: '1' }
  const signature = `${filePath}.sig`

  await writeFile(join(root, 'macos', 'other.app.tar.gz.sig'), 'unrelated')
  assert.throws(() => collectUpdaterAssets(options), /Missing or empty updater signature/)
  await mkdir(signature)
  assert.throws(() => collectUpdaterAssets(options), /Missing or empty updater signature/)
  const second = await createTempFile('macos/The Pair.app.tar.gz', '')
  const next = { ...options, bundleRoot: second.root, outDir: join(second.root, 'dist') }
  assert.throws(() => collectUpdaterAssets(next), /Empty updater asset/)
  await writeFile(second.filePath, 'artifact')
  await writeFile(`${second.filePath}.sig`, '  \n')
  assert.throws(() => collectUpdaterAssets(next), /Missing or empty updater signature/)
  await writeFile(`${second.filePath}.sig`, 'signature')
  await writeFile(join(second.root, 'macos', 'duplicate.app.tar.gz'), 'artifact')
  assert.throws(() => collectUpdaterAssets(next), /Expected exactly one updater asset.*found 2/)
  assert.deepEqual(await readdir(root), ['macos'])
  assert.deepEqual(await readdir(second.root), ['macos'])
})

test('collectUpdaterAssets rejects each platform without its updater asset', async () => {
  for (const [platform, subdir] of [
    ['macos', 'macos'],
    ['windows', 'nsis'],
    ['linux', 'appimage']
  ]) {
    const { root } = await createTempFile(`${subdir}/unrelated.txt`)
    assert.throws(
      () =>
        collectUpdaterAssets({
          platform,
          bundleRoot: root,
          outDir: join(root, 'dist'),
          version: '1'
        }),
      /Expected exactly one updater asset.*found 0/
    )
  }
})

test('collectUpdaterAssets copies Windows signatures next to the installer asset', async () => {
  const { root } = await createTempFile('nsis/The Pair_1.1.12_x64-setup.exe', 'win-exe')
  await writeFile(join(root, 'nsis', 'The Pair_1.1.12_x64-setup.exe.sig'), 'win-sig')

  const outDir = join(root, 'dist')
  const result = collectUpdaterAssets({
    platform: 'windows',
    bundleRoot: root,
    outDir,
    version: '1.1.12'
  })

  assert.equal(result.length, 2)
  assert.equal(await readFile(join(outDir, 'the-pair-1.1.12-setup.exe'), 'utf8'), 'win-exe')
  assert.equal(await readFile(join(outDir, 'the-pair-1.1.12-setup.exe.sig'), 'utf8'), 'win-sig')
})

test('collectUpdaterAssets copies Linux signatures next to the AppImage asset', async () => {
  const { root } = await createTempFile(
    'appimage/The Pair_1.1.12_amd64.AppImage.tar.gz',
    'linux-tar'
  )
  await writeFile(join(root, 'appimage', 'The Pair_1.1.12_amd64.AppImage.tar.gz.sig'), 'linux-sig')

  const outDir = join(root, 'dist')
  const result = collectUpdaterAssets({
    platform: 'linux',
    bundleRoot: root,
    outDir,
    version: '1.1.12'
  })

  assert.equal(result.length, 2)
  assert.equal(await readFile(join(outDir, 'the-pair-1.1.12.AppImage.tar.gz'), 'utf8'), 'linux-tar')
  assert.equal(
    await readFile(join(outDir, 'the-pair-1.1.12.AppImage.tar.gz.sig'), 'utf8'),
    'linux-sig'
  )
})
