import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { copyFile, mkdir, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import {
  updateCargoLockPackageVersion,
  updateCargoTomlPackageVersion,
  updatePackageLockVersion
} from '../scripts/bump-version.mjs'

const repoRoot = fileURLToPath(new URL('..', import.meta.url))

const FIXTURE_FILES = [
  'package.json',
  'package-lock.json',
  'CHANGELOG.md',
  'src-tauri/Cargo.toml',
  'src-tauri/Cargo.lock',
  'scripts/bump-version.mjs',
  'scripts/changelog.mjs'
]

async function makeFixture(): Promise<string> {
  // realpath: on macOS tmpdir() is a symlink (/var -> /private/var).
  const root = await realpath(await mkdtemp(join(tmpdir(), 'the-pair-bump-')))
  await mkdir(join(root, 'src-tauri'), { recursive: true })
  await mkdir(join(root, 'scripts'), { recursive: true })
  for (const file of FIXTURE_FILES) {
    await copyFile(join(repoRoot, file), join(root, file))
  }
  return root
}

function runBump(root: string, version: string) {
  return spawnSync(process.execPath, [join(root, 'scripts', 'bump-version.mjs'), version], {
    cwd: root,
    encoding: 'utf8'
  })
}

/** Line numbers (0-based) whose content differs; both texts must have the same line count. */
function changedLines(before: string, after: string): number[] {
  const a = before.split('\n')
  const b = after.split('\n')
  assert.equal(a.length, b.length, 'line count changed')
  return a.flatMap((line, index) => (line === b[index] ? [] : [index]))
}

async function readFixture(root: string) {
  const entries = await Promise.all(
    ['package.json', 'package-lock.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock'].map(
      async (file) => [file, await readFile(join(root, file), 'utf8')] as const
    )
  )
  return Object.fromEntries(entries) as Record<string, string>
}

test('bump updates package.json, package-lock.json, Cargo.toml and Cargo.lock in lock-step', async () => {
  const root = await makeFixture()
  try {
    const changelogPath = join(root, 'CHANGELOG.md')
    const changelog = await readFile(changelogPath, 'utf8')
    await writeFile(
      changelogPath,
      changelog.replace('## [', '## [9.9.9] - 2026-09-25\n\n### Fixed\n\n- Test\n\n## [')
    )
    const before = await readFixture(root)

    const result = runBump(root, '9.9.9')
    assert.equal(result.status, 0, result.stderr)

    const after = await readFixture(root)

    // package.json: only the version field.
    assert.equal(JSON.parse(after['package.json']).version, '9.9.9')
    assert.equal(changedLines(before['package.json'], after['package.json']).length, 1)

    // package-lock.json: the root "version" and packages[""].version only.
    const lock = JSON.parse(after['package-lock.json'])
    assert.equal(lock.version, '9.9.9')
    assert.equal(lock.packages[''].version, '9.9.9')
    assert.equal(changedLines(before['package-lock.json'], after['package-lock.json']).length, 2)

    // Cargo.toml: the [package] version line only — dependency versions untouched.
    const tomlChanges = changedLines(before['src-tauri/Cargo.toml'], after['src-tauri/Cargo.toml'])
    assert.equal(tomlChanges.length, 1)
    assert.equal(after['src-tauri/Cargo.toml'].split('\n')[tomlChanges[0]], 'version = "9.9.9"')
    const packageTable = after['src-tauri/Cargo.toml'].split(/^\[/m)[1]
    assert.match(packageTable, /^package\]\nname = "app"\nversion = "9\.9\.9"\n/)

    // Cargo.lock: the local "app" crate entry only.
    const lockChanges = changedLines(before['src-tauri/Cargo.lock'], after['src-tauri/Cargo.lock'])
    assert.equal(lockChanges.length, 1)
    const lockLines = after['src-tauri/Cargo.lock'].split('\n')
    assert.equal(lockLines[lockChanges[0]], 'version = "9.9.9"')
    assert.equal(lockLines[lockChanges[0] - 1], 'name = "app"')
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test('bump still writes every file but exits non-zero when the changelog entry is missing', async () => {
  const root = await makeFixture()
  try {
    const result = runBump(root, '9.9.8')
    assert.equal(result.status, 1)
    assert.match(result.stderr, /No changelog entry found for 9\.9\.8/)

    const after = await readFixture(root)
    assert.equal(JSON.parse(after['package.json']).version, '9.9.8')
    assert.equal(JSON.parse(after['package-lock.json']).version, '9.9.8')
    assert.match(after['src-tauri/Cargo.toml'], /^version = "9\.9\.8"$/m)
    assert.match(after['src-tauri/Cargo.lock'], /^name = "app"\nversion = "9\.9\.8"$/m)
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test('bump rejects an invalid version without touching any file', async () => {
  const root = await makeFixture()
  try {
    const before = await readFixture(root)
    const result = runBump(root, 'v1.2')
    assert.equal(result.status, 1)
    assert.deepEqual(await readFixture(root), before)
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

test('updateCargoTomlPackageVersion only edits the [package] table', () => {
  const toml = [
    '[package]',
    'name = "app"',
    'version = "0.1.0" # keep comment',
    '',
    '[package.metadata.docs]',
    'version = "7.7.7"',
    '',
    '[dependencies]',
    'serde = { version = "1.0", features = ["derive"] }',
    'version = "3.0.0"',
    ''
  ].join('\n')

  const { text, name } = updateCargoTomlPackageVersion(toml, '2.8.2')
  assert.equal(name, 'app')
  assert.equal(
    text,
    toml.replace('version = "0.1.0" # keep comment', 'version = "2.8.2" # keep comment')
  )
})

test('updateCargoTomlPackageVersion preserves CRLF and fails without a [package] version', () => {
  const crlf = '[package]\r\nname = "app"\r\nversion = "0.1.0"\r\n'
  assert.equal(
    updateCargoTomlPackageVersion(crlf, '1.2.3').text,
    '[package]\r\nname = "app"\r\nversion = "1.2.3"\r\n'
  )
  assert.throws(
    () =>
      updateCargoTomlPackageVersion(
        '[package]\nname = "app"\n\n[dependencies]\nversion = "1"\n',
        '1.2.3'
      ),
    /no `version = "\.\.\."` line in the \[package\] table/
  )
})

test('updateCargoLockPackageVersion ignores registry crates with the same name', () => {
  const lock = [
    '[[package]]',
    'name = "app"',
    'version = "0.5.0"',
    'source = "registry+https://github.com/rust-lang/crates.io-index"',
    '',
    '[[package]]',
    'name = "app"',
    'version = "0.1.0"',
    'dependencies = [',
    ' "serde",',
    ']',
    '',
    '[[package]]',
    'name = "serde"',
    'version = "1.0.0"',
    'source = "registry+https://github.com/rust-lang/crates.io-index"',
    ''
  ].join('\n')

  const updated = updateCargoLockPackageVersion(lock, 'app', '2.8.2')
  assert.deepEqual(changedLines(lock, updated), [7])
  assert.equal(updated.split('\n')[7], 'version = "2.8.2"')
  assert.throws(() => updateCargoLockPackageVersion(lock, 'missing', '2.8.2'), /found 0/)
})

test('updatePackageLockVersion keeps formatting byte-for-byte apart from the versions', () => {
  const original = `${JSON.stringify(
    {
      name: 'the-pair',
      version: '1.0.0',
      lockfileVersion: 3,
      requires: true,
      packages: {
        '': { name: 'the-pair', version: '1.0.0' },
        'node_modules/x': { version: '1.0.0' }
      }
    },
    null,
    2
  )}\n`
  const updated = updatePackageLockVersion(original, '2.0.0')
  assert.deepEqual(changedLines(original, updated), [2, 8])
  assert.ok(updated.includes('"node_modules/x": {\n      "version": "1.0.0"'))
})
