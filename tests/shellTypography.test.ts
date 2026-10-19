import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const css = await readFile(new URL('../src/renderer/src/assets/main.css', import.meta.url), 'utf8')

async function src(path: string): Promise<string> {
  return readFile(new URL(`../src/renderer/src/${path}`, import.meta.url), 'utf8')
}

// Shell chrome reads in sans; the conversation core stays mono. These tests
// pin the split so a future edit can't silently re-mono the shell or
// re-sans the terminal stream.

test('design tokens define a sans shell face and a mono core face', () => {
  const sansBlock = /--font-sans:\s*([^;]+);/.exec(css)?.[1] ?? ''
  const monoBlock = /--font-mono:\s*([^;]+);/.exec(css)?.[1] ?? ''
  assert.match(sansBlock, /Geist Sans/)
  assert.doesNotMatch(sansBlock, /Geist Mono/)
  assert.match(monoBlock, /Geist Mono/)
})

test('shell surfaces opt out of the terminal look', async () => {
  const hub = await src('components/PairsHub.tsx')
  const shell = await src('components/AppShell.tsx')
  const activity = await src('components/ShellViews.tsx')
  const palette = await src('components/CommandPalette.tsx')

  assert.match(hub, /shell-card/)
  assert.match(shell, /app-source-list/)
  assert.match(palette, /shell-card/)
  for (const [name, source] of [
    ['PairsHub', hub],
    ['ShellViews', activity],
    ['CommandPalette', palette]
  ] as Array<[string, string]>) {
    assert.doesNotMatch(
      source,
      /font-mono[^"']*text-\[1[01]px\] uppercase/,
      `${name} still sets tiny mono uppercase shell labels`
    )
  }
})

test('conversation core keeps the mono terminal stream', async () => {
  const console = await src('components/PairConsole.tsx')
  const block = await src('components/terminal/TerminalBlock.tsx')
  const diff = await src('components/inspector/InlineDiff.tsx')

  assert.match(console, /font-mono/)
  assert.match(block, /font-mono/)
  assert.match(diff, /font-mono/)
})

test('shell provides card and source-list surfaces', () => {
  assert.match(css, /\.shell-card \{/)
  assert.match(css, /\.app-source-list \{/)
  assert.match(css, /prefers-reduced-transparency/)
})

test('state, role and surface accents are registered as theme colors', () => {
  // Without an @theme entry Tailwind silently drops opacity / variant
  // utilities like `bg-state-running/15` or `hover:text-state-error`.
  const themeBlock = /@theme \{([\s\S]*?)\n\}/.exec(css)?.[1] ?? ''
  for (const token of [
    'state-running',
    'state-done',
    'state-error',
    'role-mentor',
    'role-executor',
    'role-human',
    'surface-base',
    'surface-raised',
    'surface-sunken',
    'rule',
    'rule-strong'
  ]) {
    assert.match(themeBlock, new RegExp(`--color-${token}: var\\(--${token}\\);`), token)
  }
})
