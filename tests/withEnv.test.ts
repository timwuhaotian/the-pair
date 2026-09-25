import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import { buildWindowsCommandLine, parseWithEnvArgs, quoteWindowsArg } from '../scripts/with-env.mjs'

const script = fileURLToPath(new URL('../scripts/with-env.mjs', import.meta.url))

test('parseWithEnvArgs splits leading NAME=value pairs from the command', () => {
  assert.deepEqual(
    parseWithEnvArgs(['THE_PAIR_E2E_MOCK=true', 'SCENARIO=a=b', 'npm', 'run', 'dev', 'X=1']),
    {
      env: { THE_PAIR_E2E_MOCK: 'true', SCENARIO: 'a=b' },
      command: 'npm',
      args: ['run', 'dev', 'X=1']
    }
  )
})

test('parseWithEnvArgs accepts an optional -- separator and empty values', () => {
  assert.deepEqual(parseWithEnvArgs(['EMPTY=', '--', 'wdio', 'run']), {
    env: { EMPTY: '' },
    command: 'wdio',
    args: ['run']
  })
})

test('parseWithEnvArgs requires a command', () => {
  assert.throws(() => parseWithEnvArgs(['A=1']), /Usage/)
  assert.throws(() => parseWithEnvArgs(['A=1', '--']), /Usage/)
})

test('Windows command lines quote only arguments that need it', () => {
  assert.equal(quoteWindowsArg('e2e/wdio.conf.ts'), 'e2e/wdio.conf.ts')
  assert.equal(
    quoteWindowsArg('C:\\Program Files\\nodejs\\node.exe'),
    '"C:\\Program Files\\nodejs\\node.exe"'
  )
  assert.equal(quoteWindowsArg(''), '""')
  assert.equal(quoteWindowsArg('say "hi"'), '"say \\"hi\\""')
  assert.equal(buildWindowsCommandLine('npm', ['run', 'dev']), 'npm run dev')
})

test('with-env passes the variables to the command and propagates its exit code', () => {
  const probe =
    "process.exit(process.env.WITH_ENV_A === 'one' && process.env.WITH_ENV_B === 'x=y' ? 7 : 1)"
  const result = spawnSync(
    process.execPath,
    [script, 'WITH_ENV_A=one', 'WITH_ENV_B=x=y', process.execPath, '-e', probe],
    { encoding: 'utf8' }
  )
  assert.equal(result.status, 7, result.stderr)
})

test('with-env keeps the parent environment', () => {
  const result = spawnSync(
    process.execPath,
    [
      script,
      'WITH_ENV_A=1',
      process.execPath,
      '-e',
      'process.exit(process.env.WITH_ENV_PARENT === "kept" ? 0 : 1)'
    ],
    { encoding: 'utf8', env: { ...process.env, WITH_ENV_PARENT: 'kept' } }
  )
  assert.equal(result.status, 0, result.stderr)
})
