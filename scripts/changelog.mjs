// Shared CHANGELOG.md helpers. The bump script, `npm run validate:changelog`,
// the release workflow's changelog gate and its release-notes extraction all
// go through these functions, so "has an entry" and "what the release notes
// are" can never disagree.

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/**
 * Matches the Keep a Changelog heading for `version` at the start of a line,
 * e.g. `## [2.8.2] - 2026-09-25`. `### [2.8.2]` or `## [2.8.20]` do not match.
 */
export function changelogHeadingPattern(version) {
  return new RegExp(`^## \\[${escapeRegExp(version)}\\]`)
}

const NEXT_SECTION = /^## \[/
// Keep a Changelog link reference definitions at the bottom of the file,
// e.g. `[1.0.1]: https://github.com/...`.
const LINK_REFERENCE = /^\[[^\]]+\]:\s/

function findHeadingIndex(lines, version) {
  const heading = changelogHeadingPattern(version)
  return lines.findIndex((line) => heading.test(line))
}

/**
 * Returns the body of the `## [version]` section (without its heading), or
 * `null` when there is no such section. The body ends at the next `## [`
 * heading or at the link reference definitions, whichever comes first.
 */
export function findChangelogSection(changelog, version) {
  const lines = changelog.split(/\r?\n/)
  const start = findHeadingIndex(lines, version)
  if (start < 0) {
    return null
  }

  let end = lines.length
  for (let index = start + 1; index < lines.length; index += 1) {
    if (NEXT_SECTION.test(lines[index]) || LINK_REFERENCE.test(lines[index])) {
      end = index
      break
    }
  }

  return lines
    .slice(start + 1, end)
    .join('\n')
    .trim()
}

export function hasChangelogEntry(changelog, version) {
  const section = findChangelogSection(changelog, version)
  return section !== null && section.length > 0
}

/**
 * Like `findChangelogSection`, but throws when the section is missing or
 * empty instead of silently producing wrong release notes.
 */
export function extractChangelogSection(changelog, version) {
  const section = findChangelogSection(changelog, version)
  if (section === null) {
    throw new Error(
      `CHANGELOG.md has no "## [${version}]" heading at the start of a line. Add a "## [${version}] - YYYY-MM-DD" section.`
    )
  }
  if (section.length === 0) {
    throw new Error(`CHANGELOG.md section "## [${version}]" is empty.`)
  }
  return section
}
