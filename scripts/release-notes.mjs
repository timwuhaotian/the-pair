// Builds the GitHub release body (also used as the updater manifest notes):
// a downloads table, the version's CHANGELOG.md section and, when the GitHub
// API is reachable, a list of contributors since the previous release.
//
// Usage (release workflow):
//   node scripts/release-notes.mjs --version 2.8.2 --output release-notes.md \
//     [--changelog CHANGELOG.md] [--repo owner/name --head <sha>]
//
// A missing/empty changelog section is fatal. The contributors lookup is
// best effort: any failure only drops that block.
import { execFileSync } from 'node:child_process'
import { readFileSync, realpathSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import process from 'node:process'

import { extractChangelogSection } from './changelog.mjs'

export function buildDownloadTable(version) {
  return [
    '## Downloads',
    '',
    '| Platform | File |',
    '|---|---|',
    `| **macOS** (Apple Silicon + Intel) | \`the-pair-${version}.zip\` |`,
    `| **Windows** | \`the-pair-${version}-setup.exe\` |`,
    `| **Linux** | \`the-pair-${version}.AppImage\` |`
  ].join('\n')
}

/**
 * Takes `[{ login, type }]` (the commit authors from the GitHub compare API)
 * and returns the unique human GitHub logins in first-seen order. Commits
 * whose author email is not linked to a GitHub account have no login and are
 * skipped, as are bots.
 */
export function collectContributorLogins(authors) {
  const logins = []
  for (const author of authors ?? []) {
    const login = typeof author?.login === 'string' ? author.login.trim() : ''
    if (!login || author.type === 'Bot' || login.endsWith('[bot]')) {
      continue
    }
    if (!logins.includes(login)) {
      logins.push(login)
    }
  }
  return logins
}

export function formatContributors(logins) {
  if (!logins || logins.length === 0) {
    return ''
  }
  const mentions = logins.map((login) => `@${login}`).join(', ')
  return `## Contributors\n\nThanks to ${mentions} for contributing to this release!`
}

export function buildReleaseNotes({ version, changelog, contributors = [] }) {
  const notes = extractChangelogSection(changelog, version)
  const blocks = [buildDownloadTable(version), notes, formatContributors(contributors)]
  return `${blocks.filter(Boolean).join('\n\n')}\n`
}

function gh(args) {
  return execFileSync('gh', args, {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    timeout: 60_000
  }).trim()
}

/**
 * Best effort: contributors between the latest published release and `head`.
 * Returns [] (and logs a warning) on any failure.
 */
export function fetchContributorLogins({ repo, head, version, run = gh }) {
  try {
    const previousTag = run(['api', `repos/${repo}/releases/latest`, '--jq', '.tag_name'])
    if (!previousTag || previousTag === `v${version}`) {
      return []
    }
    const authors = JSON.parse(
      run([
        'api',
        `repos/${repo}/compare/${previousTag}...${head}`,
        '--jq',
        '[.commits[] | {login: (.author.login // ""), type: (.author.type // "")}]'
      ])
    )
    return collectContributorLogins(authors)
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    console.warn(`Skipping contributors section: ${message.split('\n')[0]}`)
    return []
  }
}

function parseArgs(argv) {
  const options = {}
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index]
    if (!token.startsWith('--')) {
      continue
    }
    const next = argv[index + 1]
    if (next === undefined || next.startsWith('--')) {
      throw new Error(`Missing value for ${token}`)
    }
    options[token.slice(2)] = next
    index += 1
  }
  return options
}

function main() {
  const options = parseArgs(process.argv.slice(2))
  const version = options.version
  const output = options.output ?? 'release-notes.md'
  const changelogPath = options.changelog ?? 'CHANGELOG.md'

  if (!version) {
    throw new Error(
      'Usage: node scripts/release-notes.mjs --version <version> [--output <path>] [--changelog <path>] [--repo <owner/name> --head <sha>]'
    )
  }

  const changelog = readFileSync(changelogPath, 'utf8')
  const contributors =
    options.repo && options.head
      ? fetchContributorLogins({ repo: options.repo, head: options.head, version })
      : []

  writeFileSync(output, buildReleaseNotes({ version, changelog, contributors }))
  console.log(
    `Wrote release notes for ${version} to ${output}` +
      (contributors.length > 0 ? ` (${contributors.length} contributor(s))` : '')
  )
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
