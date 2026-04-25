import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

const updateNotificationSource = await readFile(
  new URL('../src/renderer/src/components/UpdateNotification.tsx', import.meta.url),
  'utf8'
)

const appSource = await readFile(new URL('../src/renderer/src/App.tsx', import.meta.url), 'utf8')

test('update release notes render only through markdown in the modal', () => {
  const markdownReleaseBodyRenders = updateNotificationSource.match(
    /<ReactMarkdown\b[^>]*>\{releaseBody\}<\/ReactMarkdown>/g
  )
  const sourceWithoutMarkdownRender = updateNotificationSource.replace(
    /<ReactMarkdown\b[^>]*>\{releaseBody\}<\/ReactMarkdown>/g,
    ''
  )

  assert.equal(markdownReleaseBodyRenders?.length, 1)
  assert.doesNotMatch(sourceWithoutMarkdownRender, />\s*\{releaseBody\}\s*</)
})

test('update check stores release notes separately from the status message', () => {
  assert.match(appSource, /setReleaseBody\(update\.body \|\| null\)/)
  assert.match(appSource, /setMessage\(`Version \$\{update\.version\} is available`\)/)
  assert.doesNotMatch(appSource, /setMessage\(update\.body/)
})
