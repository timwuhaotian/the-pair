import assert from 'node:assert/strict'
import { readdir, readFile } from 'node:fs/promises'
import test from 'node:test'

const componentsDir = new URL('../src/renderer/src/components/', import.meta.url)
const read = (path: string): Promise<string> => readFile(new URL(path, componentsDir), 'utf8')

const localeNames = ['en', 'zh', 'ja', 'ko'] as const

type LocaleTree = { [key: string]: string | LocaleTree }

async function loadLocale(name: string): Promise<LocaleTree> {
  const raw = await readFile(
    new URL(`../src/renderer/src/locales/${name}.json`, import.meta.url),
    'utf8'
  )
  return JSON.parse(raw) as LocaleTree
}

function flatKeys(tree: LocaleTree, prefix = ''): string[] {
  return Object.entries(tree).flatMap(([key, value]) =>
    typeof value === 'string' ? [prefix + key] : flatKeys(value, `${prefix}${key}.`)
  )
}

test('per-pair panels are keyed by pair id so drafts never leak across pairs', async () => {
  const dashboard = await read('Dashboard.tsx')
  assert.match(dashboard, /<PairConsole key=\{selectedPair\.id\}/)
  assert.match(dashboard, /<PairOperationsPanel\s+key=\{selectedPair\.id\}/)
})

test('operations panel is visible at the 1200px minimum window width', async () => {
  const dashboard = await read('Dashboard.tsx')
  assert.doesNotMatch(dashboard, /hidden w-\[320px\] shrink-0 xl:flex/)
  assert.match(dashboard, /lg:flex lg:flex-col/)
})

test('every t() key used by the renderer exists in all four locales', async () => {
  const sources: string[] = []
  const walk = async (dir: URL): Promise<void> => {
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      const url = new URL(entry.name + (entry.isDirectory() ? '/' : ''), dir)
      if (entry.isDirectory()) await walk(url)
      else if (/\.tsx?$/.test(entry.name)) sources.push(await readFile(url, 'utf8'))
    }
  }
  await walk(new URL('../src/renderer/src/', import.meta.url))
  const used = new Set<string>()
  for (const source of sources) {
    for (const match of source.matchAll(/\bt\(\s*['"]([a-zA-Z]+\.[a-zA-Z0-9_.]+)['"]/g)) {
      used.add(match[1])
    }
  }
  assert.ok(used.has('pair.thinking'))
  assert.ok(!used.has('common.thinking'))

  for (const name of localeNames) {
    const keys = new Set(flatKeys(await loadLocale(name)))
    const missing = [...used].filter(
      (key) => !keys.has(key) && !keys.has(`${key}_one`) && !keys.has(`${key}_other`)
    )
    assert.deepEqual(missing, [], `${name} is missing keys`)
  }
})

test('delete confirmation copy is localized and explains worktree preservation (C1)', async () => {
  const app = await readFile(new URL('../src/renderer/src/App.tsx', import.meta.url), 'utf8')
  assert.match(app, /t\('modals\.deleteTitle'/)
  assert.match(app, /t\('modals\.deleteWorktreeNote'\)/)
  assert.doesNotMatch(app, /message="This will permanently remove/)
  for (const name of localeNames) {
    const locale = await loadLocale(name)
    const note = (locale.modals as LocaleTree).deleteWorktreeNote as string
    assert.match(note, /git stash/)
    assert.match(note, /the-pair\//)
  }
})

test('restoring a task is guarded while the pair is busy', async () => {
  const app = await readFile(new URL('../src/renderer/src/App.tsx', import.meta.url), 'utf8')
  const panel = await read('PairOperationsPanel.tsx')
  const modal = await read('AssignTaskModal.tsx')
  assert.match(app, /handleRestoreTask[\s\S]*?isPairBusy\(selectedPair\.status\)/)
  assert.match(panel, /restoreDisabled=\{isPairBusy\(pair\.status\)\}/)
  assert.match(modal, /useState\(\(\) => restoringSpec\?\.spec \?\? ''\)/)
})
