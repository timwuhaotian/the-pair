import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

const readSource = (path: string): Promise<string> =>
  readFile(new URL(`../${path}`, import.meta.url), 'utf8')

const staticImportPattern = (specifier: string): RegExp =>
  new RegExp(`import\\s+(?:[^'"]+?\\s+from\\s+)?['"]${specifier.replaceAll('/', '\\/')}['"]`)

test('App keeps pair detail and heavy modal components out of the static startup path', async () => {
  const source = await readSource('src/renderer/src/App.tsx')
  const dashboard = await readSource('src/renderer/src/components/Dashboard.tsx')

  for (const specifier of [
    './components/PairConsole',
    './components/PairOperationsPanel',
    './components/CreatePairModal',
    './components/AssignTaskModal',
    './components/PairSettingsModal'
  ]) {
    assert.doesNotMatch(source, staticImportPattern(specifier))
  }

  assert.doesNotMatch(source, /function\s+PairConsole\b/)
  assert.match(dashboard, /lazy\(\(\)\s*=>\s*import\(['"]\.\/PairConsole['"]\)/)
  assert.match(dashboard, /lazy\(\(\)\s*=>\s*import\(['"]\.\/PairOperationsPanel['"]\)/)
  assert.match(source, /lazy\(\(\)\s*=>\s*import\(['"]\.\/components\/CommandPalette['"]\)/)
})

test('hub list components stay out of the static startup path', async () => {
  const hub = await readSource('src/renderer/src/components/PairsHub.tsx')
  const shellViews = await readSource('src/renderer/src/components/ShellViews.tsx')

  for (const specifier of [
    './PairConsole',
    './PairOperationsPanel',
    './TaskHistoryPanel',
    './TimelinePanel'
  ]) {
    assert.doesNotMatch(hub, staticImportPattern(specifier))
    assert.doesNotMatch(shellViews, staticImportPattern(specifier))
  }
})
