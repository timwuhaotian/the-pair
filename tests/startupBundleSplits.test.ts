import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

const readSource = (path: string): Promise<string> =>
  readFile(new URL(`../${path}`, import.meta.url), 'utf8')

const staticImportPattern = (specifier: string): RegExp =>
  new RegExp(`import\\s+(?:[^'"]+?\\s+from\\s+)?['"]${specifier.replaceAll('/', '\\/')}['"]`)

test('App keeps pair detail and heavy modal components out of the static startup path', async () => {
  const source = await readSource('src/renderer/src/App.tsx')

  for (const specifier of [
    './components/PairDetail',
    './components/OnboardingWizard',
    './components/CreatePairModal',
    './components/AssignTaskModal',
    './components/PairSettingsModal'
  ]) {
    assert.doesNotMatch(source, staticImportPattern(specifier))
  }

  assert.doesNotMatch(source, /function\s+PairDetail\b/)
  assert.match(source, /lazy\(\(\)\s*=>\s*import\(['"]\.\/components\/PairDetail['"]\)/)
})

test('App paints the shell while startup data is still loading', async () => {
  const source = await readSource('src/renderer/src/App.tsx')

  assert.doesNotMatch(source, /if\s*\(\s*isInitializing\s*\)\s*{\s*return\s+null\s*}/)
  assert.match(source, /\bpairsLoaded\b/)
  assert.match(source, /\bmodelsLoading\b/)
})

test('App defers automatic production update checks until after first paint', async () => {
  const source = await readSource('src/renderer/src/App.tsx')

  assert.doesNotMatch(
    source,
    /if\s*\(\s*import\.meta\.env\.PROD\s*\)\s*{\s*void performUpdateCheck\(\)\s*}/
  )
  assert.match(source, /queueStartupUpdateCheck/)
})

test('timeline and task history defer report export code until export actions run', async () => {
  const timelinePanel = await readSource('src/renderer/src/components/TimelinePanel.tsx')
  const taskHistoryPanel = await readSource('src/renderer/src/components/TaskHistoryPanel.tsx')

  for (const source of [timelinePanel, taskHistoryPanel]) {
    assert.doesNotMatch(source, staticImportPattern('../lib/reportExport'))
    assert.match(source, /await\s+import\(['"]\.\.\/lib\/reportExport['"]\)/)
  }
})

test('update notification defers markdown rendering until release notes are opened', async () => {
  const source = await readSource('src/renderer/src/components/UpdateNotification.tsx')

  assert.doesNotMatch(source, staticImportPattern('react-markdown'))
  assert.doesNotMatch(source, staticImportPattern('remark-gfm'))
  assert.match(source, /lazy\(\(\)\s*=>\s*import\(['"]\.\/MarkdownContent['"]\)/)
})
