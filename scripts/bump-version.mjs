#!/usr/bin/env node
import { readFileSync, writeFileSync } from 'fs'
import { fileURLToPath } from 'url'
import { dirname, join } from 'path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const rootDir = join(__dirname, '..')

const newVersion = process.argv[2]

if (!newVersion || !/^\d+\.\d+\.\d+$/.test(newVersion)) {
  console.error('Usage: npm run bump <version>')
  console.error('Example: npm run bump 1.1.20')
  process.exit(1)
}

const pkgPath = join(rootDir, 'package.json')
const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'))
const oldVersion = pkg.version

console.log(`Bumping version: ${oldVersion} → ${newVersion}`)

// Update package.json
pkg.version = newVersion
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n')

// Check changelog
const changelogPath = join(rootDir, 'CHANGELOG.md')
const changelog = readFileSync(changelogPath, 'utf8')

if (!changelog.includes(`## [${newVersion}]`)) {
  console.error(`\n⚠️  WARNING: No changelog entry found for ${newVersion}`)
  console.error(`\nPlease add to CHANGELOG.md:\n`)
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
