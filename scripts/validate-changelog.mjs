#!/usr/bin/env node
// Fails unless CHANGELOG.md has a non-empty `## [<package.json version>]`
// section. The release workflow runs this as its changelog gate, and it uses
// the same matcher as the release-notes extraction (scripts/changelog.mjs).
import { readFileSync } from 'fs'
import { fileURLToPath } from 'url'
import { dirname, join } from 'path'

import { findChangelogSection } from './changelog.mjs'

const __dirname = dirname(fileURLToPath(import.meta.url))
const rootDir = join(__dirname, '..')

const pkg = JSON.parse(readFileSync(join(rootDir, 'package.json'), 'utf8'))
const version = pkg.version

const changelog = readFileSync(join(rootDir, 'CHANGELOG.md'), 'utf8')
const section = findChangelogSection(changelog, version)

if (section === null || section.length === 0) {
  if (section === null) {
    console.error(`❌ Missing changelog entry for version ${version}`)
    console.error(`\nCHANGELOG.md needs a heading at the start of a line:\n`)
  } else {
    console.error(`❌ Changelog entry for version ${version} is empty`)
    console.error(`\nAdd release notes under the heading:\n`)
  }
  console.error(`## [${version}] - ${new Date().toISOString().split('T')[0]}\n`)
  console.error(`### Fixed\n- Fixed ...\n`)
  process.exit(1)
}

console.log(`✅ Changelog entry found for version ${version}`)
