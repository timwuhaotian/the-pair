#!/usr/bin/env node
// `npm run bump <version>` — sets the app version everywhere it is recorded:
//   - package.json                 (source of truth; tauri.conf.json reads it via "../package.json")
//   - package-lock.json            (root "version" and packages[""].version)
//   - src-tauri/Cargo.toml         ([package] version only — never a dependency's)
//   - src-tauri/Cargo.lock         (the root crate's [[package]] entry)
// Every file is computed before anything is written, so a failure leaves the
// tree untouched. Afterwards it checks that CHANGELOG.md has the new section.
import { existsSync, readFileSync, realpathSync, writeFileSync } from 'fs'
import { fileURLToPath } from 'url'
import { dirname, join } from 'path'

import { hasChangelogEntry } from './changelog.mjs'

const VERSION_PATTERN = /^\d+\.\d+\.\d+$/

function detectEol(text) {
  return text.includes('\r\n') ? '\r\n' : '\n'
}

function stringifyJsonLike(original, value) {
  const eol = detectEol(original)
  const body = JSON.stringify(value, null, 2).replace(/\n/g, eol)
  return original.endsWith('\n') ? `${body}${eol}` : body
}

export function updatePackageJsonVersion(text, version) {
  const pkg = JSON.parse(text)
  const previous = pkg.version
  pkg.version = version
  return { text: stringifyJsonLike(text, pkg), previous }
}

export function updatePackageLockVersion(text, version) {
  const lock = JSON.parse(text)
  lock.version = version
  if (lock.packages && lock.packages[''] && typeof lock.packages[''] === 'object') {
    lock.packages[''].version = version
  }
  return stringifyJsonLike(text, lock)
}

const TABLE_HEADER = /^\s*\[\[?\s*([^\]]+?)\s*\]\]?\s*(#.*)?$/
const VERSION_LINE = /^(\s*version\s*=\s*)"[^"]*"(.*)$/
const NAME_LINE = /^\s*name\s*=\s*"([^"]*)"/

/**
 * Replaces the `version = "..."` line of the `[package]` table and returns the
 * crate name. Lines in any other table (dependencies, lib, ...) are left alone.
 */
export function updateCargoTomlPackageVersion(text, version) {
  const eol = detectEol(text)
  const lines = text.split(eol)
  let section = null
  let name = null
  let versionLine = -1

  for (let index = 0; index < lines.length; index += 1) {
    const header = lines[index].match(TABLE_HEADER)
    if (header) {
      section = header[1]
      continue
    }
    if (section !== 'package') {
      continue
    }
    const nameMatch = lines[index].match(NAME_LINE)
    if (nameMatch && name === null) {
      name = nameMatch[1]
    }
    if (versionLine < 0 && VERSION_LINE.test(lines[index])) {
      versionLine = index
    }
  }

  if (versionLine < 0) {
    throw new Error('Cargo.toml: no `version = "..."` line in the [package] table')
  }
  if (!name) {
    throw new Error('Cargo.toml: no `name = "..."` line in the [package] table')
  }

  lines[versionLine] = lines[versionLine].replace(VERSION_LINE, `$1"${version}"$2`)
  return { text: lines.join(eol), name }
}

/**
 * Updates the version of the workspace's own crate in Cargo.lock: the
 * `[[package]]` entry named `crateName` that has no `source` (registry/git
 * crates always have one).
 */
export function updateCargoLockPackageVersion(text, crateName, version) {
  const eol = detectEol(text)
  const lines = text.split(eol)

  const blocks = []
  for (let index = 0; index < lines.length; index += 1) {
    if (lines[index].trim() === '[[package]]') {
      blocks.push({ start: index, end: lines.length })
      if (blocks.length > 1) {
        blocks[blocks.length - 2].end = index
      }
    }
  }

  const matches = blocks.filter(({ start, end }) => {
    const body = lines.slice(start + 1, end)
    const named = body.some((line) => line.match(NAME_LINE)?.[1] === crateName)
    const hasSource = body.some((line) => /^\s*source\s*=/.test(line))
    return named && !hasSource
  })

  if (matches.length !== 1) {
    throw new Error(
      `Cargo.lock: expected exactly one local [[package]] named "${crateName}", found ${matches.length}`
    )
  }

  const { start, end } = matches[0]
  const versionLine = lines.slice(start + 1, end).findIndex((line) => VERSION_LINE.test(line))
  if (versionLine < 0) {
    throw new Error(`Cargo.lock: [[package]] "${crateName}" has no version line`)
  }
  const index = start + 1 + versionLine
  lines[index] = lines[index].replace(VERSION_LINE, `$1"${version}"$2`)
  return lines.join(eol)
}

/**
 * Computes and writes every version file under `rootDir`. Returns the
 * previous package.json version and the list of files written.
 */
export function bumpVersion(rootDir, version) {
  if (!VERSION_PATTERN.test(version)) {
    throw new Error(`Invalid version "${version}" (expected X.Y.Z)`)
  }

  const paths = {
    packageJson: join(rootDir, 'package.json'),
    packageLock: join(rootDir, 'package-lock.json'),
    cargoToml: join(rootDir, 'src-tauri', 'Cargo.toml'),
    cargoLock: join(rootDir, 'src-tauri', 'Cargo.lock')
  }

  const writes = []

  const packageJson = updatePackageJsonVersion(readFileSync(paths.packageJson, 'utf8'), version)
  writes.push([paths.packageJson, packageJson.text])

  if (existsSync(paths.packageLock)) {
    writes.push([
      paths.packageLock,
      updatePackageLockVersion(readFileSync(paths.packageLock, 'utf8'), version)
    ])
  }

  const cargoToml = updateCargoTomlPackageVersion(readFileSync(paths.cargoToml, 'utf8'), version)
  writes.push([paths.cargoToml, cargoToml.text])

  if (existsSync(paths.cargoLock)) {
    writes.push([
      paths.cargoLock,
      updateCargoLockPackageVersion(readFileSync(paths.cargoLock, 'utf8'), cargoToml.name, version)
    ])
  }

  for (const [path, text] of writes) {
    writeFileSync(path, text)
  }

  return { previous: packageJson.previous, files: writes.map(([path]) => path) }
}

function main() {
  const rootDir = join(dirname(fileURLToPath(import.meta.url)), '..')
  const newVersion = process.argv[2]

  if (!newVersion || !VERSION_PATTERN.test(newVersion)) {
    console.error('Usage: npm run bump <version>')
    console.error('Example: npm run bump 1.1.20')
    process.exit(1)
  }

  const { previous, files } = bumpVersion(rootDir, newVersion)

  console.log(`Bumping version: ${previous} → ${newVersion}`)
  for (const file of files) {
    console.log(`  updated ${file.slice(rootDir.length + 1)}`)
  }

  const changelog = readFileSync(join(rootDir, 'CHANGELOG.md'), 'utf8')

  if (!hasChangelogEntry(changelog, newVersion)) {
    console.error(`\n⚠️  WARNING: No changelog entry found for ${newVersion}`)
    console.error(`\nPlease add to CHANGELOG.md (heading at the start of a line):\n`)
    console.error(`## [${newVersion}] - ${new Date().toISOString().split('T')[0]}\n`)
    console.error(`### Fixed`)
    console.error(`- Fixed ...\n`)
    console.error(
      `Then run: git add -A && git commit -m "chore: bump version to ${newVersion}" && git push`
    )
    process.exit(1)
  }

  console.log(`✅ Version bumped to ${newVersion}`)
  console.log(`✅ Changelog entry exists`)
  console.log(`\nNext steps:`)
  console.log(`  git add -A`)
  console.log(`  git commit -m "chore: bump version to ${newVersion}"`)
  console.log(`  git push`)
  console.log(`\n⚠️  IMPORTANT: Do NOT create or push tags manually!`)
  console.log(`   The GitHub Actions workflow will auto-detect the version bump and publish.`)
}

function isMainModule() {
  try {
    return (
      Boolean(process.argv[1]) && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)
    )
  } catch {
    return false
  }
}

if (isMainModule()) {
  try {
    main()
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exit(1)
  }
}
