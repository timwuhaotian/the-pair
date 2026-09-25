import assert from 'node:assert/strict'
import test from 'node:test'

import {
  changelogHeadingPattern,
  extractChangelogSection,
  findChangelogSection,
  hasChangelogEntry
} from '../scripts/changelog.mjs'
import {
  buildReleaseNotes,
  collectContributorLogins,
  fetchContributorLogins,
  formatContributors
} from '../scripts/release-notes.mjs'

const changelog = [
  '# Changelog',
  '',
  'All notable changes to this project will be documented in this file.',
  '',
  '## [2.8.20] - 2026-10-01',
  '',
  '- Twenty',
  '',
  '## [2.8.2] - 2026-09-25',
  '',
  '### Fixed',
  '',
  '- Two point eight point two',
  '',
  '## [2.8.1] - 2026-09-23',
  '',
  '- Previous',
  '',
  '[2.8.1]: https://github.com/example/the-pair/compare/v2.8.0...v2.8.1',
  '[2.8.0]: https://github.com/example/the-pair/releases/tag/v2.8.0',
  ''
].join('\n')

test('changelog heading pattern is anchored and exact', () => {
  const pattern = changelogHeadingPattern('2.8.2')
  assert.ok(pattern.test('## [2.8.2] - 2026-09-25'))
  assert.ok(!pattern.test('### [2.8.2] - 2026-09-25'))
  assert.ok(!pattern.test(' ## [2.8.2]'))
  assert.ok(!pattern.test('## [2.8.20]'))
  assert.ok(!pattern.test('## [2x8x2]'))
})

test('extractChangelogSection returns only the requested section body', () => {
  assert.equal(
    extractChangelogSection(changelog, '2.8.2'),
    '### Fixed\n\n- Two point eight point two'
  )
  assert.equal(extractChangelogSection(changelog, '2.8.20'), '- Twenty')
})

test('the last section stops at the link reference definitions', () => {
  assert.equal(extractChangelogSection(changelog, '2.8.1'), '- Previous')
})

test('a heading typo is a missing entry for both the gate and the extraction', () => {
  const typo = changelog.replace('## [2.8.2]', '### [2.8.2]')
  assert.equal(hasChangelogEntry(typo, '2.8.2'), false)
  assert.equal(findChangelogSection(typo, '2.8.2'), null)
  assert.throws(() => extractChangelogSection(typo, '2.8.2'), /no "## \[2\.8\.2\]" heading/)
})

test('an empty section fails instead of producing empty release notes', () => {
  const empty = '# Changelog\n\n## [3.0.0] - 2026-10-01\n\n## [2.8.2] - 2026-09-25\n\n- x\n'
  assert.equal(hasChangelogEntry(empty, '3.0.0'), false)
  assert.throws(() => extractChangelogSection(empty, '3.0.0'), /is empty/)
})

test('CRLF changelogs are handled', () => {
  const crlf = changelog.replace(/\n/g, '\r\n')
  assert.equal(hasChangelogEntry(crlf, '2.8.2'), true)
  assert.equal(extractChangelogSection(crlf, '2.8.2'), '### Fixed\n\n- Two point eight point two')
})

test('buildReleaseNotes puts the downloads table before the changelog section', () => {
  const notes = buildReleaseNotes({ version: '2.8.2', changelog })
  assert.ok(notes.startsWith('## Downloads\n'))
  assert.ok(notes.includes('`the-pair-2.8.2.zip`'))
  assert.ok(notes.includes('`the-pair-2.8.2-setup.exe`'))
  assert.ok(notes.includes('`the-pair-2.8.2.AppImage`'))
  assert.ok(
    notes.includes(
      '| **Linux** | `the-pair-2.8.2.AppImage` |\n\n### Fixed\n\n- Two point eight point two\n'
    )
  )
  assert.ok(!notes.includes('# Changelog'))
  assert.ok(!notes.includes('Previous'))
  assert.ok(!notes.includes('Contributors'))
  assert.ok(notes.endsWith('\n'))
})

test('buildReleaseNotes refuses to build notes without a matching section', () => {
  assert.throws(
    () => buildReleaseNotes({ version: '9.9.9', changelog }),
    /no "## \[9\.9\.9\]" heading/
  )
})

test('buildReleaseNotes appends a contributors block when logins are known', () => {
  const notes = buildReleaseNotes({ version: '2.8.2', changelog, contributors: ['alice', 'bob'] })
  assert.ok(
    notes.endsWith(
      '- Two point eight point two\n\n## Contributors\n\nThanks to @alice, @bob for contributing to this release!\n'
    )
  )
})

test('collectContributorLogins keeps unique human logins in order', () => {
  assert.deepEqual(
    collectContributorLogins([
      { login: 'alice', type: 'User' },
      { login: '', type: '' },
      { login: 'dependabot[bot]', type: 'Bot' },
      { login: 'github-actions', type: 'Bot' },
      { login: 'bob', type: 'User' },
      { login: 'alice', type: 'User' }
    ]),
    ['alice', 'bob']
  )
  assert.equal(formatContributors([]), '')
})

test('fetchContributorLogins compares the latest release with the release commit', () => {
  const calls: string[][] = []
  const run = (args: string[]): string => {
    calls.push(args)
    if (args[1] === 'repos/example/the-pair/releases/latest') {
      return 'v2.8.1'
    }
    return JSON.stringify([
      { login: 'alice', type: 'User' },
      { login: 'renovate[bot]', type: 'Bot' }
    ])
  }

  const logins = fetchContributorLogins({
    repo: 'example/the-pair',
    head: 'abc123',
    version: '2.8.2',
    run
  })

  assert.deepEqual(logins, ['alice'])
  assert.equal(calls[1][1], 'repos/example/the-pair/compare/v2.8.1...abc123')
})

test('fetchContributorLogins never throws (the block is best effort)', () => {
  const originalWarn = console.warn
  console.warn = () => {}
  try {
    const logins = fetchContributorLogins({
      repo: 'example/the-pair',
      head: 'abc123',
      version: '2.8.2',
      run: () => {
        throw new Error('HTTP 404')
      }
    })
    assert.deepEqual(logins, [])
  } finally {
    console.warn = originalWarn
  }
})

test('fetchContributorLogins skips the lookup when the latest release is this version', () => {
  const logins = fetchContributorLogins({
    repo: 'example/the-pair',
    head: 'abc123',
    version: '2.8.2',
    run: () => 'v2.8.2'
  })
  assert.deepEqual(logins, [])
})
