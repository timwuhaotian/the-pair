import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

const packageJson = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'))

test('dev smoke script uses deterministic mock provider scenario', () => {
  assert.equal(
    packageJson.scripts['dev:smoke'],
    'THE_PAIR_E2E_MOCK=true THE_PAIR_E2E_MOCK_SCENARIO=dev-smoke npm run dev'
  )
})
