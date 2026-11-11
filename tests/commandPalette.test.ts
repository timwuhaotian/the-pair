import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const actions = await readFile(
  new URL('../src/renderer/src/lib/paletteActions.ts', import.meta.url),
  'utf8'
)

const palette = await readFile(
  new URL('../src/renderer/src/components/CommandPalette.tsx', import.meta.url),
  'utf8'
)

const app = await readFile(new URL('../src/renderer/src/App.tsx', import.meta.url), 'utf8')

const menu = await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8')

const enLocale = await readFile(
  new URL('../src/renderer/src/locales/en.json', import.meta.url),
  'utf8'
)

test('palette actions cover pair, navigation, view, and app groups', () => {
  assert.match(actions, /export function buildPaletteActions\(/)
  assert.match(actions, /export function buildPairJumpActions\(/)
  for (const id of [
    'pair.new',
    'task.new',
    'pair.pause',
    'pair.resume',
    'pair.settings',
    'pair.clearSession',
    'nav.pairs',
    'nav.activity',
    'nav.models',
    'nav.insights',
    'view.toggleSidebar',
    'view.toggleTheme',
    'view.shortcuts',
    'app.checkUpdates'
  ]) {
    assert.match(actions, new RegExp(`id: '${id}'`), `missing action ${id}`)
  }
})

test('palette gates context-only actions behind availability', () => {
  assert.match(actions, /available\?: \(\) => boolean/)
  assert.match(actions, /available: \(\) => selectedPair !== null/)
})

test('CommandPalette supports fuzzy filter with keyboard selection', () => {
  assert.match(palette, /export function CommandPalette\(/)
  assert.match(palette, /new Fuse/)
  assert.match(palette, /data-testid="command-palette"/)
  assert.match(palette, /data-testid="command-palette-input"/)
  assert.match(palette, /ArrowDown/)
  assert.match(palette, /aria-modal/)
})

const chrome = await readFile(
  new URL('../src/renderer/src/components/AppChrome.tsx', import.meta.url),
  'utf8'
)

test('App wires Cmd+K, section jumps, and back navigation', () => {
  assert.match(app, /key: 'k'/)
  assert.match(app, /shortcuts\.commandPalette/)
  assert.match(app, /key: '\['/)
  assert.match(app, /shortcuts\.goBack/)
  assert.match(app, /shortcuts\.goPairs/)
  assert.match(app, /shortcuts\.goActivity/)
  assert.match(app, /shortcuts\.goModels/)
  assert.match(app, /shortcuts\.goInsights/)
  assert.match(app, /<CommandPalette/)
  assert.match(app, /onOpenPalette=/)
  assert.match(chrome, /chrome-palette/)
  assert.match(chrome, /onOpenPalette/)
})

test('App subscribes to native menu events with the same handlers', () => {
  for (const event of [
    'app:menu:new-pair',
    'app:menu:new-task',
    'app:menu:pause',
    'app:menu:resume',
    'app:menu:back',
    'app:menu:go-pairs',
    'app:menu:go-activity',
    'app:menu:go-insights',
    'app:menu:toggle-sidebar',
    'app:menu:toggle-theme',
    'app:menu:shortcuts',
    'app:menu:palette'
  ]) {
    assert.match(app, new RegExp(`'${event}'`), `missing subscription ${event}`)
  }
})

test('Rust menu provides App, File, Edit, View, Window, Help menus', () => {
  for (const name of ['"The Pair"', '"File"', '"Edit"', '"View"', '"Window"', '"Help"']) {
    assert.match(menu, new RegExp(`SubmenuBuilder::new\\(app, ${name}\\)`), `missing ${name} menu`)
  }
  assert.match(menu, /\.about\(None\)/)
  assert.match(menu, /\.quit\(\)/)
  assert.match(menu, /\.fullscreen\(\)/)
  assert.match(menu, /\.minimize\(\)/)
})

test('Rust menu emits frontend events for custom actions', () => {
  for (const id of ['menu:new-pair', 'menu:toggle-sidebar', 'menu:palette', 'menu:shortcuts']) {
    assert.match(menu, new RegExp(`"${id}"`), `missing menu id ${id}`)
  }
  assert.match(menu, /app:menu:palette/)
})

test('palette and shortcut copy is localized', () => {
  const locale = JSON.parse(enLocale)
  assert.equal(typeof locale.palette.title, 'string')
  assert.equal(typeof locale.palette.placeholder, 'string')
  assert.equal(typeof locale.palette.groups.pair, 'string')
  assert.equal(typeof locale.shortcuts.commandPalette, 'string')
  assert.equal(typeof locale.shortcuts.goBack, 'string')
})
