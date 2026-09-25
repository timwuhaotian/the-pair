import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

const packageJson = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'))
const scripts: Record<string, string> = packageJson.scripts

test('dev smoke script uses deterministic mock provider scenario', () => {
  assert.equal(
    scripts['dev:smoke'],
    'node scripts/with-env.mjs THE_PAIR_E2E_MOCK=true THE_PAIR_E2E_MOCK_SCENARIO=dev-smoke npm run dev'
  )
})

test('mock-mode scripts set their env through the cross-platform wrapper', () => {
  assert.equal(scripts['dev:mock'], 'node scripts/with-env.mjs THE_PAIR_E2E_MOCK=true npm run dev')
  assert.equal(
    scripts.e2e,
    'node scripts/with-env.mjs THE_PAIR_E2E_MOCK=true wdio run e2e/wdio.conf.ts'
  )
})

test('no npm script uses a POSIX-only `VAR=value command` prefix (breaks under cmd.exe)', () => {
  const posixEnvPrefix = /(^|&&|\|\||;)\s*[A-Za-z_][A-Za-z0-9_]*=\S*\s/
  const offenders = Object.entries(scripts)
    .filter(([, command]) => posixEnvPrefix.test(command))
    .map(([name]) => name)
  assert.deepEqual(offenders, [])
})
